use super::{AgentRunCoordinator, AgentRunRequest, RunRoom, SystemFollowup};
use crate::control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig};
use crate::routes::ApiError;
use crate::runs::{RunLedger, RunRecord, RunSource, RunStart, RunStatus};
use crate::state::DaemonState;
use anima_core::{
    AgentConfig, AgentConfigUpdate, AgentRuntime, AgentSettings, AgentStatus, Content, DataValue,
    Message, MessageRole, ModelAdapter, ModelGenerateRequest, ModelGenerateResponse,
    ModelStopReason, TokenUsage,
};
use async_trait::async_trait;
use axum::http::StatusCode;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};

#[tokio::test]
async fn failed_task_memory_save_does_not_publish_or_leak_into_later_save() {
    let blocked_path = snapshot_path("memory-write-failure");
    std::fs::create_dir_all(&blocked_path).unwrap();
    let memory = Arc::new(RwLock::new(anima_memory::MemoryManager::new()));
    let embeddings = Arc::new(RwLock::new(
        crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
    ));
    let result = anima_core::TaskResult::success(
        Content {
            text: "uncommitted task memory".into(),
            ..Content::default()
        },
        0,
    );
    super::persist_task_result_memory(
        &result,
        "worker",
        "Worker",
        memory.clone(),
        embeddings,
        Some(crate::memory_store::MemoryStoreConfig::Json(
            blocked_path.clone(),
        )),
    )
    .await;
    assert_eq!(
        memory.read().await.size(),
        0,
        "failed memory must not become visible"
    );
    std::fs::remove_dir(&blocked_path).unwrap();
    let store = crate::memory_store::MemoryStoreConfig::Json(blocked_path.clone());
    crate::memory_store::save_memory_manager(Some(&store), &*memory.read().await)
        .await
        .unwrap();
    let restored = crate::memory_store::load_memory_snapshot(&store)
        .await
        .unwrap()
        .unwrap();
    assert!(
        restored.memories.is_empty(),
        "a later save must not persist the rejected memory"
    );
    std::fs::remove_file(blocked_path).unwrap();
}

struct PeerModelAdapter;
#[async_trait]
impl ModelAdapter for PeerModelAdapter {
    fn provider(&self) -> &str {
        "peer-test"
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        assert!(config.allows_tool("send_message"));
        if config.name == "Alice"
            && !request
                .messages
                .iter()
                .any(|message| message.role == MessageRole::Tool)
        {
            let mut response = model_response("Asking Bob");
            response.stop_reason = ModelStopReason::ToolCall;
            response.tool_calls = Some(vec![anima_core::ToolCall {
                id: "peer-1".into(),
                name: "send_message".into(),
                args: BTreeMap::from([
                    ("to_agent_name".into(), DataValue::String("Bob".into())),
                    (
                        "message".into(),
                        DataValue::String("Review this plan".into()),
                    ),
                ]),
            }]);
            return Ok(response);
        }
        Ok(model_response(if config.name == "Bob" {
            "Peer review completed"
        } else {
            "Peer response received"
        }))
    }
}

#[tokio::test]
async fn independent_agents_exchange_attributed_messages_in_separate_rooms() {
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(Arc::new(
        PeerModelAdapter,
    ))));
    let alice = state
        .write()
        .await
        .create_agent(test_config("Alice"))
        .unwrap()
        .state;
    let bob = state
        .write()
        .await
        .create_agent(test_config("Bob"))
        .unwrap()
        .state;
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let mut direct = request(&alice.id, "Ask Bob for a review");
    direct.room = RunRoom::Stable(format!("direct:{}", alice.id));
    let result = coordinator.run(direct).await.unwrap();
    assert_eq!(result.result.status, "success");
    let bob_after = state.read().await.get_agent(&bob.id).unwrap();
    assert!(bob_after
        .messages
        .iter()
        .any(|m| m.content.text == "Peer review completed"));
    assert!(bob_after
        .messages
        .iter()
        .all(|m| m.room_id == format!("peer:{}:{}", alice.id, bob.id)));
    let input = bob_after
        .messages
        .iter()
        .find(|m| m.role == MessageRole::User)
        .unwrap();
    assert!(input
        .content
        .metadata
        .as_ref()
        .unwrap()
        .contains_key("communication"));
    let result = coordinator
        .send_peer(
            bob.id.clone(),
            alice.id.clone(),
            "Please review my work".into(),
            anima_core::AgentCommunicationRoute::start(&bob.id),
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.result.status, "success");
    let alice_after = state.read().await.get_agent(&alice.id).unwrap();
    assert!(alice_after
        .messages
        .iter()
        .any(|m| m.room_id == format!("direct:{}", alice.id)));
    assert!(alice_after
        .messages
        .iter()
        .any(|m| m.room_id == format!("peer:{}:{}", bob.id, alice.id)));
    assert_eq!(alice_after.state.config.tools, alice.config.tools);
}

struct HelperModelAdapter {
    configs: StdMutex<Vec<AgentConfig>>,
}

#[async_trait]
impl ModelAdapter for HelperModelAdapter {
    fn provider(&self) -> &str {
        "helper-test"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.configs.lock().unwrap().push(config.clone());
        if config.name == "Companion"
            && !request.messages.iter().any(|m| m.role == MessageRole::Tool)
        {
            let mut response = model_response("Starting a helper");
            response.stop_reason = ModelStopReason::ToolCall;
            response.tool_calls = Some(vec![helper_call("Research", "Check the requested facts")]);
            return Ok(response);
        }
        Ok(model_response(if config.name == "Companion" {
            "Helper result received"
        } else {
            "Helper task completed"
        }))
    }
}

fn helper_call(name: &str, task: &str) -> anima_core::ToolCall {
    anima_core::ToolCall {
        id: "spawn-helper-1".into(),
        name: "spawn_helper".into(),
        args: BTreeMap::from([
            ("name".into(), DataValue::String(name.into())),
            ("task".into(), DataValue::String(task.into())),
        ]),
    }
}

async fn helper_lead(coordinator: &AgentRunCoordinator) -> anima_core::AgentState {
    let mut config = test_config("Companion");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["calculate", "send_message"])
            .unwrap(),
    );
    config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("workspaceRole".into(), DataValue::String("lead".into()));
    coordinator
        .state
        .write()
        .await
        .create_agent(config)
        .unwrap()
        .state
}

async fn execute_helper_tool(
    coordinator: &AgentRunCoordinator,
    caller: &anima_core::AgentState,
    can_delegate: bool,
    name: &str,
) -> anima_core::TaskResult<Content> {
    let mut caller = caller.clone();
    caller
        .config
        .tools
        .get_or_insert_with(Vec::new)
        .push(anima_core::ToolDescriptor {
            name: "spawn_helper".into(),
            description: String::new(),
            parameters_schema: BTreeMap::new(),
            examples: None,
        });
    let context = crate::tools::ToolExecutionContext::new(
        Arc::new(RwLock::new(anima_memory::MemoryManager::new())),
        Arc::new(RwLock::new(
            crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
        )),
        None,
        crate::tools::ToolRegistry::new(),
        crate::tools::new_shared_process_manager_with_limit(1),
        None,
        None,
    )
    .with_team(coordinator.clone(), can_delegate);
    let input = message(
        &caller.id,
        "helper-test",
        "Do a bounded task",
        MessageRole::User,
    );
    context
        .execute_tool(caller, input, helper_call(name, "Do a bounded task"))
        .await
}

#[tokio::test]
async fn spawn_helper_from_one_companion_reuses_and_persists_restricted_helpers() {
    let adapter = Arc::new(HelperModelAdapter {
        configs: StdMutex::new(vec![]),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(3)));
    let lead = helper_lead(&coordinator).await;
    let path = snapshot_path("spawn-helper");
    let store = ControlPlaneStoreConfig::Json(path.clone());
    state
        .write()
        .await
        .set_control_plane_store(Some(store.clone()));
    for _ in 0..6 {
        let result = coordinator
            .run(request(&lead.id, "Ask a helper to check facts"))
            .await
            .unwrap();
        assert_eq!(result.result.status, "success");
    }
    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    assert_eq!(
        saved.agents.len(),
        2,
        "single companion must create one reusable helper"
    );
    let helper = saved.agents.iter().find(|a| a.state.id != lead.id).unwrap();
    let settings = helper.state.config.settings.as_ref().unwrap();
    assert_eq!(
        settings.additional.get("workspaceRole"),
        Some(&DataValue::String("helper".into()))
    );
    assert_eq!(
        settings.additional.get("parentAgentId"),
        Some(&DataValue::String(lead.id.clone()))
    );
    assert_eq!(helper.state.config.model, lead.config.model);
    assert_eq!(helper.state.config.provider, lead.config.provider);
    assert!(settings.max_tool_iterations.is_some_and(|limit| limit <= 8));
    assert!(helper
        .messages
        .iter()
        .any(|m| m.content.text == "Helper task completed"));
    let lead_after = saved.agents.iter().find(|a| a.state.id == lead.id).unwrap();
    assert!(lead_after
        .messages
        .iter()
        .any(|m| m.role == MessageRole::Tool && m.content.text.contains("Helper task completed")));
    for config in adapter
        .configs
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.name != "Companion")
    {
        assert!(config.allows_tool("calculate"));
        for forbidden in [
            "spawn_helper",
            "delegate_to_agent",
            "send_message",
            "broadcast_message",
        ] {
            assert!(
                !config.allows_tool(forbidden),
                "helper received {forbidden}"
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn spawn_helper_process_tools_are_not_inherited_from_operate_access() {
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(Arc::new(
        HelperModelAdapter {
            configs: StdMutex::new(vec![]),
        },
    ))));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let mut lead = helper_lead(&coordinator).await;
    lead.config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors([
                "calculate",
                "read_file",
                "write_file",
                "bash",
                "bg_start",
                "bg_output",
                "bg_stop",
                "bg_list",
            ])
            .unwrap(),
    );
    state
        .write()
        .await
        .restore_agent_config(&lead.id, lead.config.clone());
    assert_eq!(
        execute_helper_tool(&coordinator, &lead, true, "Operate helper")
            .await
            .status,
        anima_core::TaskStatus::Success
    );
    let agents = state.read().await.list_agents();
    let helper = agents
        .iter()
        .find(|agent| agent.state.id != lead.id)
        .unwrap();
    for tool in ["bash", "bg_start", "bg_output", "bg_stop", "bg_list"] {
        assert!(
            !helper.state.config.allows_tool(tool),
            "helper must not inherit {tool}"
        );
    }
    for tool in ["calculate", "read_file", "write_file"] {
        assert!(helper.state.config.allows_tool(tool));
    }
    assert_eq!(
        state.read().await.get_agent(&lead.id).unwrap().state.config,
        lead.config
    );
}

#[tokio::test]
async fn spawn_helper_process_reconfiguration_is_rejected_before_execution() {
    let adapter = Arc::new(HelperModelAdapter {
        configs: StdMutex::new(vec![]),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let mut lead = helper_lead(&coordinator).await;
    lead.config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["bash", "bg_start", "bg_output", "bg_stop", "bg_list"])
            .unwrap(),
    );
    state
        .write()
        .await
        .restore_agent_config(&lead.id, lead.config.clone());
    let mut config = test_config("Reconfigured helper");
    config.tools = lead.config.tools.clone();
    config.settings.as_mut().unwrap().additional = BTreeMap::from([
        ("workspaceRole".into(), DataValue::String("helper".into())),
        ("parentAgentId".into(), DataValue::String(lead.id.clone())),
    ]);
    let helper = state.write().await.create_agent(config).unwrap().state;
    let mut task = request(&helper.id, "Try the explicitly granted process tools");
    task.room = RunRoom::Delegated { parent_id: lead.id };
    let error = coordinator.run(task).await.unwrap_err();
    assert!(error
        .message()
        .contains("Process tools are unavailable to helpers"));
    assert!(adapter.configs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn helper_process_tools_are_denied_even_when_explicitly_configured() {
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(Arc::new(
        HelperModelAdapter {
            configs: StdMutex::new(vec![]),
        },
    ))));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let mut lead = helper_lead(&coordinator).await;
    lead.config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["bash", "bg_start", "bg_output", "bg_stop", "bg_list"])
            .unwrap(),
    );
    state
        .write()
        .await
        .restore_agent_config(&lead.id, lead.config.clone());
    let mut helper = lead.clone();
    helper.id = "helper-with-explicit-process-grants".into();
    helper.config.settings.as_mut().unwrap().additional = BTreeMap::from([
        ("workspaceRole".into(), DataValue::String("helper".into())),
        ("parentAgentId".into(), DataValue::String(lead.id.clone())),
    ]);
    let context = crate::tools::ToolExecutionContext::new(
        Arc::new(RwLock::new(anima_memory::MemoryManager::new())),
        Arc::new(RwLock::new(
            crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
        )),
        None,
        crate::tools::ToolRegistry::new(),
        crate::tools::new_shared_process_manager_with_limit(1),
        None,
        None,
    )
    .with_team(coordinator, false)
    .with_delegated_parent(Some(lead.id));
    for name in ["bash", "bg_start", "bg_output", "bg_stop", "bg_list"] {
        let result = context
            .clone()
            .execute_tool(
                helper.clone(),
                message(
                    &helper.id,
                    "helper-process",
                    "Process tool request",
                    MessageRole::User,
                ),
                anima_core::ToolCall {
                    id: format!("denied-{name}"),
                    name: name.into(),
                    args: BTreeMap::new(),
                },
            )
            .await;
        assert_eq!(result.status, anima_core::TaskStatus::Error);
        assert_eq!(
            result.error.as_deref(),
            Some(
                "Process tools are unavailable to helpers until process cancellation is supported"
            )
        );
    }
}

#[tokio::test]
async fn spawn_helper_rejects_non_lead_recursive_and_saturated_calls_without_creating_agents() {
    let adapter = Arc::new(HelperModelAdapter {
        configs: StdMutex::new(vec![]),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(1)));
    let lead = helper_lead(&coordinator).await;
    let worker = state
        .write()
        .await
        .create_agent(test_config("Worker"))
        .unwrap()
        .state;
    let recursive = execute_helper_tool(&coordinator, &lead, false, "Recursive").await;
    assert!(recursive.error.unwrap().contains("recursive"));
    let denied = execute_helper_tool(&coordinator, &worker, true, "Escalation").await;
    assert!(denied.error.unwrap().contains("companion"));
    let _permit = coordinator.try_admit().unwrap();
    let busy = execute_helper_tool(&coordinator, &lead, true, "Saturated").await;
    assert!(busy.error.unwrap().contains("concurrent"));
    assert_eq!(state.read().await.list_agents().len(), 2);
    assert!(adapter.configs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn spawn_helper_failed_creation_save_rolls_back_without_running_or_leaking() {
    let adapter = Arc::new(HelperModelAdapter {
        configs: StdMutex::new(vec![]),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let lead = helper_lead(&coordinator).await;
    let path = snapshot_path("helper-persist-failure");
    std::fs::create_dir(&path).unwrap();
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(path.clone())));
    let failed = execute_helper_tool(&coordinator, &lead, true, "Must roll back").await;
    assert!(failed.error.unwrap().contains("persist"));
    assert_eq!(state.read().await.list_agents().len(), 1);
    assert!(adapter.configs.lock().unwrap().is_empty());
    std::fs::remove_dir(&path).unwrap();
    let succeeded = execute_helper_tool(&coordinator, &lead, true, "Retry").await;
    assert_eq!(succeeded.status, anima_core::TaskStatus::Success);
    let saved = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.agents.len(), 2);
    assert!(saved
        .agents
        .iter()
        .all(|a| a.state.name != "Must roll back"));
    let helper = saved
        .agents
        .iter()
        .find(|agent| agent.state.id != lead.id)
        .unwrap();
    let blocked = snapshot_path("helper-reuse-persist-failure");
    std::fs::create_dir(&blocked).unwrap();
    state
        .write()
        .await
        .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(blocked.clone())));
    let reuse_failed = execute_helper_tool(&coordinator, &lead, true, "Must not rename").await;
    assert!(reuse_failed.error.unwrap().contains("persist"));
    assert_eq!(
        state
            .read()
            .await
            .get_agent(&helper.state.id)
            .unwrap()
            .state
            .config,
        helper.state.config
    );
    std::fs::remove_dir(blocked).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn spawn_helper_refreshes_reused_permissions_and_rejects_direct_helper_runs() {
    let adapter = Arc::new(HelperModelAdapter {
        configs: StdMutex::new(vec![]),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let lead = helper_lead(&coordinator).await;
    assert_eq!(
        execute_helper_tool(&coordinator, &lead, true, "First")
            .await
            .status,
        anima_core::TaskStatus::Success
    );
    state
        .write()
        .await
        .update_agent(
            &lead.id,
            AgentConfigUpdate {
                tools: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        execute_helper_tool(&coordinator, &lead, true, "Next task")
            .await
            .status,
        anima_core::TaskStatus::Success
    );
    assert_eq!(state.read().await.list_agents().len(), 2);
    let helper = state
        .read()
        .await
        .list_agents()
        .into_iter()
        .find(|a| a.state.id != lead.id)
        .unwrap();
    assert!(!helper.state.config.allows_tool("calculate"));
    assert!(!adapter
        .configs
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .allows_tool("calculate"));
    assert!(coordinator
        .run(request(&helper.state.id, "Bypass parent permissions"))
        .await
        .is_err());
    assert!(coordinator
        .delegate(
            &lead,
            helper.state.id,
            "Bypass start allowance".into(),
            None
        )
        .await
        .unwrap_err()
        .contains("spawn_helper"));
}

#[tokio::test]
async fn spawn_helper_atomically_caps_busy_helpers_and_reuses_slots() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(GateModelAdapter {
        calls: AtomicUsize::new(0),
        entered: entered.clone(),
        release: release.clone(),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(adapter)));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(8)));
    let lead = helper_lead(&coordinator).await;
    let mut tasks = vec![];
    for index in 0..4 {
        let coordinator = coordinator.clone();
        let lead = lead.clone();
        tasks.push(tokio::spawn(async move {
            execute_helper_tool(&coordinator, &lead, true, &format!("Helper {index}")).await
        }));
        tokio::time::timeout(Duration::from_secs(3), entered.acquire())
            .await
            .expect("helper should start")
            .unwrap()
            .forget();
    }
    let full = execute_helper_tool(&coordinator, &lead, true, "Overflow").await;
    assert!(full.error.unwrap().contains("busy"));
    assert_eq!(state.read().await.list_agents().len(), 5);
    release.add_permits(5);
    for task in tasks {
        assert_eq!(task.await.unwrap().status, anima_core::TaskStatus::Success);
    }
    assert_eq!(
        execute_helper_tool(&coordinator, &lead, true, "Another task")
            .await
            .status,
        anima_core::TaskStatus::Success
    );
    assert_eq!(state.read().await.list_agents().len(), 5);
}

struct BurstHelperModelAdapter;

#[async_trait]
impl ModelAdapter for BurstHelperModelAdapter {
    fn provider(&self) -> &str {
        "burst-helper-test"
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        if config.name == "Companion"
            && !request.messages.iter().any(|m| m.role == MessageRole::Tool)
        {
            let mut response = model_response("Starting bounded subtasks");
            response.stop_reason = ModelStopReason::ToolCall;
            response.tool_calls = Some(
                (0..5)
                    .map(|index| {
                        let mut call = helper_call("Helper", "A bounded task");
                        call.id = format!("helper-{index}");
                        call
                    })
                    .collect(),
            );
            return Ok(response);
        }
        Ok(model_response("Completed bounded work"))
    }
}

#[tokio::test]
async fn spawn_helper_limits_starts_per_run_and_renews_the_allowance_for_future_runs() {
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(Arc::new(
        BurstHelperModelAdapter,
    ))));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(8)));
    let lead = helper_lead(&coordinator).await;
    for round in 1..=2 {
        coordinator
            .run(request(&lead.id, "Run five subtasks"))
            .await
            .unwrap();
        let agents = state.read().await.list_agents();
        let completed: usize = agents
            .iter()
            .filter(|a| a.state.id != lead.id)
            .map(|a| {
                a.messages
                    .iter()
                    .filter(|m| {
                        m.role == MessageRole::Assistant
                            && m.content.text == "Completed bounded work"
                    })
                    .count()
            })
            .sum();
        assert_eq!(completed, round * 4);
        let parent = agents.iter().find(|a| a.state.id == lead.id).unwrap();
        assert_eq!(
            parent
                .messages
                .iter()
                .filter(|m| m.role == MessageRole::Tool
                    && m.content.text.contains("four-helper start limit"))
                .count(),
            round
        );
    }
}

#[tokio::test]
async fn spawn_helper_timeout_releases_capacity_and_saves_failed_state() {
    let adapter = Arc::new(GateModelAdapter {
        calls: AtomicUsize::new(0),
        entered: Arc::new(Semaphore::new(0)),
        release: Arc::new(Semaphore::new(0)),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(adapter)));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(1)));
    let mut lead = helper_lead(&coordinator).await;
    lead.config.settings.as_mut().unwrap().timeout_ms = Some(5);
    state
        .write()
        .await
        .restore_agent_config(&lead.id, lead.config.clone());
    for _ in 0..2 {
        let result = execute_helper_tool(&coordinator, &lead, true, "Slow helper").await;
        assert!(result.data.unwrap().text.contains("Helper task timed out"));
        let agents = state.read().await.list_agents();
        assert_eq!(agents.len(), 2);
        let helper = agents.iter().find(|a| a.state.id != lead.id).unwrap();
        assert_eq!(helper.state.status, AgentStatus::Failed);
        assert!(!coordinator.is_agent_busy(&helper.state.id));
    }
}

/// Residual fix R3 (scoped re-review of the M3 fix wave): the model call
/// can answer before the helper deadline while an evaluator that runs
/// after it (no cancellation checkpoint exists past a model call that
/// already answered) is still blocked when the deadline elapses. If the
/// run then finishes inside the S2-H cancel grace, the parent must get
/// that reply, not "Helper task timed out", and no failure is recorded
/// over a run the runtime already completed. The daemon wires no
/// evaluator itself, so the test injects one that blocks on the memory
/// write lock.
#[tokio::test]
async fn a_helper_finishing_inside_the_grace_keeps_its_success_result() {
    let entered = Arc::new(Semaphore::new(0));
    let proceed = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(SignalThenAnswerAdapter {
        entered: entered.clone(),
        proceed: proceed.clone(),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(adapter)));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let mut lead = helper_lead(&coordinator).await;
    lead.config.settings.as_mut().unwrap().timeout_ms = Some(50);
    state
        .write()
        .await
        .restore_agent_config(&lead.id, lead.config.clone());
    let memory = state.read().await.memory_handle();
    state.write().await.test_evaluators = vec![Arc::new(MemoryLockEvaluator {
        memory: memory.clone(),
    })];

    let running = {
        let coordinator = coordinator.clone();
        let lead = lead.clone();
        tokio::spawn(
            async move { execute_helper_tool(&coordinator, &lead, true, "Slow helper").await },
        )
    };
    // The helper's provider context (a memory *read*) and its top-of-loop
    // stop checkpoint have already passed once the model call is entered;
    // only now do we take memory's write lock, so it is the evaluator (run
    // right after the model answers, with no cancellation checkpoint before
    // its own completion) that blocks on it, not anything earlier.
    tokio::time::timeout(Duration::from_secs(3), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    let held = memory.write().await;
    proceed.add_permits(1);
    // The 50ms deadline elapses while the evaluator is genuinely stuck on
    // the held lock; release it well inside the 2s grace.
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(held);

    let result = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();

    // The tool call itself always "succeeds" by returning a JSON envelope
    // describing the helper's own run outcome; that envelope is the
    // signal under test here.
    let envelope: serde_json::Value =
        serde_json::from_str(&result.data.unwrap().text).expect("a JSON envelope");
    assert_eq!(envelope["status"], "success", "envelope: {envelope}");
    assert!(envelope["error"].is_null(), "envelope: {envelope}");
    assert_eq!(envelope["result"]["text"], "Helper task completed");

    let helper = state
        .read()
        .await
        .list_agents()
        .into_iter()
        .find(|agent| agent.state.id != lead.id)
        .unwrap();
    assert_ne!(
        helper.state.status,
        AgentStatus::Failed,
        "a success inside the grace must not be turned into a failure"
    );
}

/// Signals `entered` as soon as it is called, then waits for the test's
/// go-ahead (`proceed`) before answering. Used to grab the memory lock
/// only once the model call is genuinely in flight, past the provider
/// context read and the top-of-loop stop checkpoint.
struct SignalThenAnswerAdapter {
    entered: Arc<Semaphore>,
    proceed: Arc<Semaphore>,
}

/// Test-only evaluator that blocks on the memory write lock after the
/// model has answered, then accepts.
struct MemoryLockEvaluator {
    memory: crate::state::SharedMemoryStore,
}

#[async_trait]
impl anima_core::Evaluator for MemoryLockEvaluator {
    fn name(&self) -> &str {
        "memory-lock"
    }

    fn description(&self) -> &str {
        "Blocks on the memory write lock after the model answers"
    }

    async fn validate(&self, _runtime: &AgentRuntime, _message: &Message) -> Result<bool, String> {
        Ok(true)
    }

    async fn evaluate(
        &self,
        _runtime: &AgentRuntime,
        _message: &Message,
        _response: &Content,
    ) -> Result<anima_core::EvaluatorResult, String> {
        drop(self.memory.write().await);
        Ok(anima_core::EvaluatorResult::default())
    }
}

#[async_trait]
impl ModelAdapter for SignalThenAnswerAdapter {
    fn provider(&self) -> &str {
        "signal-then-answer"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        _request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.entered.add_permits(1);
        self.proceed
            .acquire()
            .await
            .expect("proceed semaphore should remain open")
            .forget();
        Ok(model_response("Helper task completed"))
    }
}

/// Final fix wave S2-H (review A, Minor 4): a helper whose deadline
/// passes mid tool batch is cancelled and given a grace period, so the
/// tool call it asked for gets its result before the run is dropped; its
/// result still says it timed out.
#[tokio::test]
async fn a_helper_timing_out_mid_tool_batch_leaves_no_call_without_a_result() {
    assert_eq!(super::HELPER_CANCEL_GRACE_MS, 2_000);
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(Arc::new(
        RevokedHelperToolAdapter {
            entered: entered.clone(),
            release: release.clone(),
        },
    ))));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let mut lead = helper_lead(&coordinator).await;
    lead.config.settings.as_mut().unwrap().timeout_ms = Some(300);
    state
        .write()
        .await
        .restore_agent_config(&lead.id, lead.config.clone());
    let running = {
        let coordinator = coordinator.clone();
        let lead = lead.clone();
        tokio::spawn(
            async move { execute_helper_tool(&coordinator, &lead, true, "Slow tool").await },
        )
    };
    tokio::time::timeout(Duration::from_secs(3), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    // The helper's tool checks its companion's permission under the state
    // lock: held here, the tool is still running when the deadline passes.
    let held = state.write().await;
    release.add_permits(1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    drop(held);

    let result = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();
    assert!(result.data.unwrap().text.contains("Helper task timed out"));
    let helper = state
        .read()
        .await
        .list_agents()
        .into_iter()
        .find(|agent| agent.state.id != lead.id)
        .unwrap();
    let calls: Vec<String> = helper
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::Assistant)
        .filter_map(
            |message| match message.content.metadata.as_ref()?.get("toolCalls")? {
                DataValue::Array(calls) => Some(calls.clone()),
                _ => None,
            },
        )
        .flatten()
        .filter_map(|call| match call {
            DataValue::Object(call) => match call.get("id") {
                Some(DataValue::String(id)) => Some(id.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["calculate-1"], "the helper asked for its tool");
    for call in &calls {
        assert!(
            helper
                .messages
                .iter()
                .any(|message| message.role == MessageRole::Tool
                    && message
                        .content
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("toolCallId"))
                        == Some(&DataValue::String(call.clone()))),
            "{call} has its result"
        );
    }
    assert_eq!(helper.state.status, AgentStatus::Failed);
}

struct RevokedHelperToolAdapter {
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

#[async_trait]
impl ModelAdapter for RevokedHelperToolAdapter {
    fn provider(&self) -> &str {
        "revoked-helper-test"
    }
    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        if !request.messages.iter().any(|m| m.role == MessageRole::Tool) {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
            let mut response = model_response("Try the previously allowed tool");
            response.stop_reason = ModelStopReason::ToolCall;
            response.tool_calls = Some(vec![anima_core::ToolCall {
                id: "calculate-1".into(),
                name: "calculate".into(),
                args: BTreeMap::from([("expression".into(), DataValue::String("1 + 2".into()))]),
            }]);
            return Ok(response);
        }
        Ok(model_response("The tool result was returned"))
    }
}

#[tokio::test]
async fn spawn_helper_checks_current_parent_authority_before_each_tool_action() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(Arc::new(
        RevokedHelperToolAdapter {
            entered: entered.clone(),
            release: release.clone(),
        },
    ))));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let lead = helper_lead(&coordinator).await;
    let running = {
        let coordinator = coordinator.clone();
        let lead = lead.clone();
        tokio::spawn(async move {
            execute_helper_tool(&coordinator, &lead, true, "Revoked grant").await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    state
        .write()
        .await
        .update_agent(
            &lead.id,
            AgentConfigUpdate {
                tools: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
    release.add_permits(1);
    running.await.unwrap();
    let helper = state
        .read()
        .await
        .list_agents()
        .into_iter()
        .find(|a| a.state.id != lead.id)
        .unwrap();
    assert!(helper.messages.iter().any(|m| m.role == MessageRole::Tool
        && m.content.text.contains("manager no longer has permission")));
}

struct TeamModelAdapter {
    target: StdMutex<String>,
    configs: StdMutex<Vec<AgentConfig>>,
    systems: StdMutex<Vec<String>>,
}

#[async_trait]
impl ModelAdapter for TeamModelAdapter {
    fn provider(&self) -> &str {
        "team-test"
    }
    async fn generate(
        &self,
        config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.configs.lock().unwrap().push(config.clone());
        self.systems.lock().unwrap().push(request.system.clone());
        if config.name == "Manager"
            && !request
                .messages
                .iter()
                .any(|message| message.role == MessageRole::Tool)
        {
            let mut response = model_response("Delegating the draft");
            response.stop_reason = ModelStopReason::ToolCall;
            response.tool_calls = Some(vec![anima_core::ToolCall {
                id: "delegate-1".into(),
                name: "delegate_to_agent".into(),
                args: BTreeMap::from([
                    (
                        "agent_id".into(),
                        DataValue::String(self.target.lock().unwrap().clone()),
                    ),
                    (
                        "task".into(),
                        DataValue::String("Draft a content plan".into()),
                    ),
                ]),
            }]);
            return Ok(response);
        }
        Ok(model_response(if config.name == "Manager" {
            "Specialist result received"
        } else {
            "Content plan completed"
        }))
    }
}

#[tokio::test]
async fn manager_delegates_real_work_and_restores_config() {
    let adapter = Arc::new(TeamModelAdapter {
        target: StdMutex::new(String::new()),
        configs: StdMutex::new(vec![]),
        systems: StdMutex::new(vec![]),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let path = snapshot_path("delegated-team");
    let store = ControlPlaneStoreConfig::Json(path.clone());
    state
        .write()
        .await
        .set_control_plane_store(Some(store.clone()));
    let mut manager = test_config("Manager");
    manager.tools = Some(vec![crate::tools::ToolRegistry::new()
        .descriptor("send_message")
        .unwrap()]);
    manager
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("workspaceRole".into(), DataValue::String("lead".into()));
    let manager = state.write().await.create_agent(manager).unwrap().state;
    let mut worker_config = test_config("Alice");
    worker_config.tools = manager.config.tools.clone();
    let worker = state
        .write()
        .await
        .create_agent(worker_config)
        .unwrap()
        .state;
    *adapter.target.lock().unwrap() = worker.id.clone();
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(2)));
    let result = coordinator
        .run(request(&manager.id, "Ask Alice to draft a plan"))
        .await
        .unwrap();
    assert_eq!(result.result.status, "success");
    let guard = state.read().await;
    let manager_after = guard.get_agent(&manager.id).unwrap();
    let worker_after = guard.get_agent(&worker.id).unwrap();
    assert!(
        worker_after
            .messages
            .iter()
            .any(|m| m.content.text == "Content plan completed"),
        "manager messages: {:?}",
        manager_after.messages
    );
    assert!(manager_after
        .messages
        .iter()
        .any(|m| m.role == MessageRole::Tool && m.content.text.contains("Content plan completed")));
    assert_eq!(manager_after.state.config.system, manager.config.system);
    assert_eq!(manager_after.state.config.tools, manager.config.tools);
    assert_eq!(worker_after.state.config.tools, worker.config.tools);
    let persisted = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    assert!(persisted
        .agents
        .iter()
        .find(|snapshot| snapshot.state.id == worker.id)
        .unwrap()
        .messages
        .iter()
        .any(|message| message.content.text == "Content plan completed"));
    let _ = std::fs::remove_file(path);
    let configs = adapter.configs.lock().unwrap();
    assert_eq!(configs[0].system, manager.config.system);
    assert!(adapter.systems.lock().unwrap()[0].contains("Alice"));
    assert!(configs[0].allows_tool("delegate_to_agent"));
    assert!(!configs
        .iter()
        .find(|config| config.name == "Alice")
        .unwrap()
        .allows_tool("send_message"));
    assert!(!configs
        .iter()
        .find(|config| config.name == "Alice")
        .unwrap()
        .allows_tool("delegate_to_agent"));
}

#[tokio::test]
async fn delegation_rejects_self_missing_target_escalation_and_non_manager() {
    let captures = Arc::new(StdMutex::new(vec![]));
    let (coordinator, worker_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: captures.clone(),
        }),
        2,
    )
    .await;
    let mut config = test_config("Manager");
    config
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("workspaceRole".into(), DataValue::String("lead".into()));
    let manager = coordinator
        .state
        .write()
        .await
        .create_agent(config)
        .unwrap()
        .state;
    let worker = coordinator
        .state
        .read()
        .await
        .get_agent(&worker_id)
        .unwrap()
        .state;
    assert!(coordinator
        .delegate(&manager, manager.id.clone(), "self".into(), None)
        .await
        .is_err());
    assert!(coordinator
        .delegate(&worker, manager.id.clone(), "reverse".into(), None)
        .await
        .is_err());
    assert!(coordinator
        .delegate(&manager, "missing".into(), "missing".into(), None)
        .await
        .is_err());
    let mut held = Vec::new();
    for room in ["busy-1", "busy-2", "busy-3"] {
        held.push(
            coordinator
                .admit(&worker_id, room, super::AdmitMode::TryNow)
                .await
                .expect("worker has a free slot"),
        );
    }
    assert!(tokio::time::timeout(
        Duration::from_secs(1),
        coordinator.delegate(&manager, worker_id.clone(), "busy".into(), None)
    )
    .await
    .unwrap()
    .is_err());
    drop(held);
    assert!(
        !coordinator
            .parent_allows_tool(&manager.id, "write_file")
            .await
    );
    let tool = crate::tools::ToolRegistry::new()
        .descriptor("write_file")
        .unwrap();
    coordinator
        .state
        .write()
        .await
        .update_agent(
            &worker_id,
            AgentConfigUpdate {
                tools: Some(vec![tool]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(coordinator
        .delegate(&manager, worker_id, "escalate".into(), None)
        .await
        .is_err());
    assert!(captures.lock().unwrap().is_empty());
}

struct GateModelAdapter {
    calls: AtomicUsize,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

struct CapturingModelAdapter {
    requests: Arc<StdMutex<Vec<ModelGenerateRequest>>>,
}

struct OrderedGateModelAdapter {
    order: StdMutex<Vec<String>>,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

#[async_trait]
impl ModelAdapter for OrderedGateModelAdapter {
    fn provider(&self) -> &str {
        "ordered-gate"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let text = request
            .messages
            .last()
            .map(|message| message.content.text.clone())
            .unwrap_or_default();
        self.order.lock().unwrap().push(text.clone());
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        Ok(model_response(&text))
    }
}

#[async_trait]
impl ModelAdapter for GateModelAdapter {
    fn provider(&self) -> &str {
        "gate"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        self.release
            .acquire()
            .await
            .expect("release semaphore should remain open")
            .forget();
        Ok(model_response(
            request
                .messages
                .last()
                .map(|message| message.content.text.as_str())
                .unwrap_or("empty"),
        ))
    }
}

#[async_trait]
impl ModelAdapter for CapturingModelAdapter {
    fn provider(&self) -> &str {
        "capturing"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.requests
            .lock()
            .expect("request capture should not be poisoned")
            .push(request.clone());
        Ok(model_response("captured"))
    }
}

#[tokio::test]
async fn same_room_runs_wait_in_acceptance_order() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(OrderedGateModelAdapter {
        order: StdMutex::new(Vec::new()),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let (coordinator, agent_id) = coordinator_with_agent(adapter.clone(), 4).await;
    let spawn_run = |text: &str| {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "room-shared", text);
        tokio::spawn(async move { coordinator.run(request).await })
    };

    let first = spawn_run("first");
    entered.acquire().await.unwrap().forget();
    let second = spawn_run("second");
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    let third = spawn_run("third");
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(adapter.order.lock().unwrap().clone(), ["first"]);

    release.add_permits(1);
    entered.acquire().await.unwrap().forget();
    release.add_permits(1);
    entered.acquire().await.unwrap().forget();
    release.add_permits(1);
    for task in [first, second, third] {
        assert!(task.await.unwrap().is_ok());
    }
    assert_eq!(
        adapter.order.lock().unwrap().clone(),
        ["first", "second", "third"]
    );
}

#[tokio::test]
async fn different_rooms_of_one_agent_run_concurrently_and_both_commit() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(GateModelAdapter {
        calls: AtomicUsize::new(0),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let (coordinator, agent_id) = coordinator_with_agent(adapter.clone(), 4).await;
    let runs = ["room-a", "room-b"].map(|room| {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, room, room);
        tokio::spawn(async move { coordinator.run(request).await })
    });

    tokio::time::timeout(Duration::from_secs(3), entered.acquire_many(2))
        .await
        .expect("both rooms enter the model concurrently")
        .unwrap()
        .forget();
    {
        let guard = coordinator.state.read().await;
        assert_eq!(guard.in_flight_runs(&agent_id), 2);
        assert_eq!(
            guard.get_agent(&agent_id).unwrap().state.status,
            AgentStatus::Running
        );
    }
    release.add_permits(2);
    for run in runs {
        run.await.unwrap().unwrap();
    }

    let guard = coordinator.state.read().await;
    let agent = guard.get_agent(&agent_id).unwrap();
    assert_eq!(agent.state.status, AgentStatus::Completed);
    for room in ["room-a", "room-b"] {
        assert_eq!(
            agent
                .messages
                .iter()
                .filter(|message| message.room_id == room)
                .count(),
            2,
            "{room} keeps its own turn"
        );
    }
    let runs = guard.runs.for_agent(&agent_id);
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|run| run.status == RunStatus::Completed));
}

#[tokio::test]
async fn with_max_runs_per_agent_clamps_to_one_and_sixty_four() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let coordinator = AgentRunCoordinator::new(state, Arc::new(Semaphore::new(1)));

    assert_eq!(
        coordinator
            .clone()
            .with_max_runs_per_agent(0)
            .max_runs_per_agent(),
        1,
        "0 (and any non-positive request) clamps up to 1 so the semaphore is never empty"
    );
    assert_eq!(
        coordinator
            .clone()
            .with_max_runs_per_agent(super::MAX_RUNS_PER_AGENT_LIMIT)
            .max_runs_per_agent(),
        super::MAX_RUNS_PER_AGENT_LIMIT,
        "the limit itself is accepted unchanged"
    );
    assert_eq!(
        coordinator
            .clone()
            .with_max_runs_per_agent(1_000_000)
            .max_runs_per_agent(),
        super::MAX_RUNS_PER_AGENT_LIMIT,
        "an oversized request clamps down to the limit so Semaphore::new can never panic"
    );
}

#[tokio::test]
async fn the_per_agent_slot_limit_queues_runs_beyond_it() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(GateModelAdapter {
        calls: AtomicUsize::new(0),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let (coordinator, agent_id) = coordinator_with_agent(adapter.clone(), 8).await;
    let coordinator = coordinator.with_max_runs_per_agent(2);
    let runs = ["room-a", "room-b", "room-c"].map(|room| {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, room, room);
        tokio::spawn(async move { coordinator.run(request).await })
    });

    tokio::time::timeout(Duration::from_secs(3), entered.acquire_many(2))
        .await
        .expect("two rooms start")
        .unwrap()
        .forget();
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        adapter.calls.load(Ordering::SeqCst),
        2,
        "the third room waits for a free slot"
    );
    release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(3), entered.acquire())
        .await
        .expect("the third room starts once a slot frees")
        .unwrap()
        .forget();
    release.add_permits(2);
    for run in runs {
        assert!(run.await.unwrap().is_ok());
    }
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn helper_agents_have_a_single_run_slot() {
    let (coordinator, worker_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;
    let lead = helper_lead(&coordinator).await;
    let helper = coordinator
        .state
        .write()
        .await
        .create_agent(super::helper_config(&lead, "Research".into()))
        .unwrap()
        .state;

    let _running = coordinator
        .admit(&helper.id, "room-1", super::AdmitMode::TryNow)
        .await
        .expect("the helper's only slot is free");
    let busy = coordinator
        .admit(&helper.id, "room-2", super::AdmitMode::TryNow)
        .await
        .err()
        .expect("a helper runs one task at a time");
    assert_eq!(busy.message(), "Specialist is busy");
    assert!(coordinator.is_agent_busy(&helper.id));

    let _first = coordinator
        .admit(&worker_id, "room-1", super::AdmitMode::TryNow)
        .await
        .expect("first worker slot");
    let _second = coordinator
        .admit(&worker_id, "room-2", super::AdmitMode::TryNow)
        .await
        .expect("other agents keep several slots");
    assert!(!coordinator.is_agent_busy(&worker_id));
}

#[test]
fn helper_config_never_copies_owner_history_tools() {
    // Ruling 1 (M3 Task 14 pre-flight audit): a helper never gets
    // search_conversations, even when the parent has it, alongside the
    // coordination and process tools it already withholds.
    let mut state = DaemonState::new();
    let mut parent_config = test_config("Companion");
    parent_config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors([
                "calculate",
                "search_conversations",
                "read_conversation_window",
            ])
            .unwrap(),
    );
    let parent = state.create_agent(parent_config).unwrap().state;

    let helper = super::helper_config(&parent, "Research".into());

    let names = helper
        .tools
        .unwrap_or_default()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["calculate"],
        "owner history tools must not reach a helper's copied tools"
    );
}

#[tokio::test]
async fn aborted_caller_does_not_cancel_the_commit_or_leak_locks_and_slots() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(GateModelAdapter {
        calls: AtomicUsize::new(0),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let agent_id = state
        .write()
        .await
        .create_agent(test_config("cancel-safe"))
        .expect("agent should be created")
        .state
        .id;
    let coordinator = AgentRunCoordinator::new(Arc::clone(&state), Arc::new(Semaphore::new(2)));

    let caller_coordinator = coordinator.clone();
    let caller_request = request(&agent_id, "first");
    let caller = tokio::spawn(async move { caller_coordinator.run(caller_request).await });
    entered
        .acquire()
        .await
        .expect("first run should enter model")
        .forget();
    caller.abort();
    assert!(
        caller
            .await
            .expect_err("caller should be aborted")
            .is_cancelled(),
        "aborting the waiter should not abort the owned run"
    );
    release.add_permits(1);

    for _ in 0..100 {
        if coordinator.lock_counts() == (0, 0)
            && state
                .read()
                .await
                .get_agent(&agent_id)
                .is_some_and(|snapshot| snapshot.state.status == AgentStatus::Completed)
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(coordinator.lock_counts(), (0, 0));
    assert!(state.read().await.agents.contains_key(&agent_id));

    let retry_coordinator = coordinator.clone();
    let retry_request = request(&agent_id, "second");
    let retry = tokio::spawn(async move { retry_coordinator.run(retry_request).await });
    entered
        .acquire()
        .await
        .expect("subsequent run should enter model")
        .forget();
    release.add_permits(1);
    assert!(retry.await.expect("retry should join").is_ok());
}

/// Runs `threads` copies of `make_and_drop_lease` in lockstep for
/// `rounds` rounds: every round, all threads rendezvous on `start` before
/// building and immediately dropping a lease, so every round's drops
/// truly race each other rather than merely interleaving loosely.
fn stress_concurrent_drops(
    threads: usize,
    rounds: usize,
    make_and_drop_lease: impl Fn() + Send + Sync + 'static,
) {
    let make_and_drop_lease = Arc::new(make_and_drop_lease);
    let start = Arc::new(std::sync::Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let make_and_drop_lease = Arc::clone(&make_and_drop_lease);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                for _ in 0..rounds {
                    start.wait();
                    make_and_drop_lease();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("racer thread should not panic");
    }
}

#[test]
fn concurrent_lease_drops_on_one_key_leave_no_stale_registry_entry() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let coordinator = AgentRunCoordinator::new(state, Arc::new(Semaphore::new(1)));

    // Many leases on the SAME (agent, room) key, dropped by many threads
    // at once, over and over: a failed try-acquire racing the holder's
    // own release is one instance of this, but any two leases whose
    // drops overlap can hit it. `SessionLease::drop`/`SlotLease::drop`
    // must decide removal atomically with releasing their own reference,
    // or a race here leaks the registry entry forever (nothing else ever
    // revisits a key once every lease on it has dropped).
    let session_coordinator = coordinator.clone();
    stress_concurrent_drops(32, 4000, move || {
        let (key, lock) = session_coordinator.session_lock("racer-agent", "racer-room");
        drop(super::SessionLease {
            key,
            lock: Some(lock),
            guard: None,
            locks: Arc::clone(&session_coordinator.session_locks),
        });
    });
    assert_eq!(
        coordinator.lock_counts(),
        (0, 0),
        "racing session-lease drops on one key must not leak a stale registry entry"
    );

    let slot_coordinator = coordinator.clone();
    stress_concurrent_drops(32, 4000, move || {
        let slots = slot_coordinator.agent_slot_semaphore("racer-agent", 1);
        drop(super::SlotLease {
            agent_id: "racer-agent".to_string(),
            slots: Some(slots),
            permit: None,
            registry: Arc::clone(&slot_coordinator.agent_slots),
        });
    });
    assert_eq!(
        coordinator.lock_counts(),
        (0, 0),
        "racing slot-lease drops on one key must not leak a stale registry entry"
    );
}

/// The waiting budget is per agent and try-only, and a unit comes back on
/// every exit of the task holding it, including cancellation and panic.
#[tokio::test]
async fn waiting_budget_is_per_agent_try_only_and_released_on_every_exit() {
    let coordinator = AgentRunCoordinator::new(
        Arc::new(RwLock::new(DaemonState::new())),
        Arc::new(Semaphore::new(1)),
    );
    let mut units: Vec<_> = (0..super::MAX_QUEUED_RUNS_PER_AGENT)
        .map(|_| {
            coordinator
                .try_take_waiting_unit("agent-x")
                .expect("within the budget")
        })
        .collect();
    assert!(
        coordinator.try_take_waiting_unit("agent-x").is_none(),
        "a ninth waiter is refused without waiting"
    );
    let other = coordinator
        .try_take_waiting_unit("agent-y")
        .expect("each agent has its own budget");

    let cancelled = {
        let unit = units.pop().unwrap();
        tokio::spawn(async move {
            let _unit = unit;
            std::future::pending::<()>().await;
        })
    };
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    let panicked = {
        let unit = units.pop().unwrap();
        tokio::spawn(async move {
            let _unit = unit;
            panic!("simulated panic in a task holding a waiting unit");
        })
    };
    assert!(panicked.await.unwrap_err().is_panic());
    assert_eq!(
        coordinator.waiting_runs("agent-x"),
        super::MAX_QUEUED_RUNS_PER_AGENT - 2
    );
    let refill = coordinator
        .try_take_waiting_unit("agent-x")
        .expect("a released unit can be taken again");

    drop((units, refill, other));
    assert!(
        coordinator.waiting_budget.lock().unwrap().is_empty(),
        "an agent with no waiting runs keeps no budget entry"
    );
}

/// No path may hold a global permit while it waits for a room lock or an
/// agent slot: room and slot are acquired first, and only then the
/// global permit (spec §4.3). A run queued behind another run in the
/// *same* room must not starve a run in a *different* room of the same
/// agent out of the global permit while it waits.
#[tokio::test]
async fn a_run_queued_for_a_room_lock_does_not_hold_the_global_permit() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(GateModelAdapter {
        calls: AtomicUsize::new(0),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    // Only 2 global permits: if a room-locked waiter held one, a third
    // room's run would have none left to take.
    let (coordinator, agent_id) = coordinator_with_agent(adapter.clone(), 2).await;

    let run_a = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "room-r", "a");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    entered
        .acquire()
        .await
        .expect("run A enters the model and holds the global permit")
        .forget();

    // Run B queues behind A on room R's lock; it must not reach (let
    // alone hold) the global permit while it waits.
    let run_b = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "room-r", "b");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        adapter.calls.load(Ordering::SeqCst),
        1,
        "run B is queued behind A on the room lock, not in the model"
    );

    // Room R2 of the SAME agent still gets the free global permit even
    // though B is queued and A has not released.
    let run_c = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "room-r2", "c");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    tokio::time::timeout(Duration::from_secs(3), entered.acquire())
        .await
        .expect("a different room of the same agent still gets the free global permit")
        .unwrap()
        .forget();

    release.add_permits(2);
    assert!(run_a.await.unwrap().is_ok());
    assert!(run_c.await.unwrap().is_ok());

    entered
        .acquire()
        .await
        .expect("B now runs once A's room lock frees")
        .forget();
    release.add_permits(1);
    assert!(run_b.await.unwrap().is_ok());

    assert_eq!(coordinator.lock_counts(), (0, 0));
}

/// The run owns its task once spawned (see `run_spawned`): a caller
/// dropped while still waiting for a room lock or slot must not cancel
/// the run itself, any more than a caller dropped while the run is
/// already in the model does (see
/// `aborted_caller_does_not_cancel_the_commit_or_leak_locks_and_slots`).
#[tokio::test]
async fn dropped_caller_does_not_cancel_a_run_still_queued_for_a_room_lock() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(GateModelAdapter {
        calls: AtomicUsize::new(0),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let (coordinator, agent_id) = coordinator_with_agent(adapter.clone(), 4).await;

    // Hold the room (and a slot) directly, with no model involved, so the
    // queued run below is blocked purely on the room lock.
    let blocker = coordinator
        .admit(&agent_id, "room-queued", super::AdmitMode::TryNow)
        .await
        .expect("the room and a slot are free");

    let caller = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "room-queued", "queued");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        adapter.calls.load(Ordering::SeqCst),
        0,
        "the run is queued on the room lock, not in the model yet"
    );

    // Drop (abort) the caller while its run is still queued for the room.
    caller.abort();
    assert!(
        caller
            .await
            .expect_err("caller should be aborted")
            .is_cancelled(),
        "aborting the waiter should not abort the owned run"
    );

    // Only now does the blocker release the room; the queued run must
    // still be there to take it, then reach and finish in the model.
    drop(blocker);
    tokio::time::timeout(Duration::from_secs(3), entered.acquire())
        .await
        .expect("the queued run still enters the model once the blocker releases")
        .unwrap()
        .forget();
    release.add_permits(1);

    for _ in 0..100 {
        if coordinator.lock_counts() == (0, 0)
            && coordinator
                .state
                .read()
                .await
                .get_agent(&agent_id)
                .is_some_and(|snapshot| snapshot.state.status == AgentStatus::Completed)
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(coordinator.lock_counts(), (0, 0));
    let guard = coordinator.state.read().await;
    let agent = guard.get_agent(&agent_id).unwrap();
    assert_eq!(agent.state.status, AgentStatus::Completed);
    assert!(
        agent
            .messages
            .iter()
            .any(|message| message.room_id == "room-queued"),
        "the queued run committed its turn"
    );
}

#[tokio::test]
async fn different_agents_run_concurrently_with_available_global_permits() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(GateModelAdapter {
        calls: AtomicUsize::new(0),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let (first_id, second_id) = {
        let mut guard = state.write().await;
        let first = guard
            .create_agent(test_config("first"))
            .expect("first agent should be created")
            .state
            .id;
        let second = guard
            .create_agent(test_config("second"))
            .expect("second agent should be created")
            .state
            .id;
        (first, second)
    };
    let coordinator = AgentRunCoordinator::new(state, Arc::new(Semaphore::new(2)));

    let first_coordinator = coordinator.clone();
    let first_request = request(&first_id, "first");
    let first = tokio::spawn(async move { first_coordinator.run(first_request).await });
    let second_coordinator = coordinator.clone();
    let second_request = request(&second_id, "second");
    let second = tokio::spawn(async move { second_coordinator.run(second_request).await });
    entered
        .acquire_many(2)
        .await
        .expect("both agents should enter the model")
        .forget();
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
    release.add_permits(2);
    assert!(first.await.expect("first task should join").is_ok());
    assert!(second.await.expect("second task should join").is_ok());
}

#[tokio::test]
async fn stable_room_passes_only_that_rooms_history_to_model() {
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let adapter = Arc::new(CapturingModelAdapter {
        requests: Arc::clone(&requests),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let agent_id = {
        let mut guard = state.write().await;
        let snapshot = guard
            .create_agent(test_config("room-agent"))
            .expect("agent should be created");
        let agent_id = snapshot.state.id.clone();
        let mut seeded = snapshot;
        seeded.messages = vec![
            message(&agent_id, "room-web", "ordinary message", MessageRole::User),
            message(
                &agent_id,
                "room-telegram",
                "telegram question",
                MessageRole::User,
            ),
            message(
                &agent_id,
                "room-telegram",
                "telegram answer",
                MessageRole::Assistant,
            ),
        ];
        seeded.message_count = seeded.messages.len();
        guard.agents.insert(
            agent_id.clone(),
            AgentRuntime::from_snapshot(seeded, adapter.clone()),
        );
        agent_id
    };
    let coordinator = AgentRunCoordinator::new(state, Arc::new(Semaphore::new(2)));

    coordinator
        .run(AgentRunRequest {
            agent_id,
            content: Content {
                text: "telegram follow-up".into(),
                ..Content::default()
            },
            room: RunRoom::Stable("room-telegram".into()),
            idempotency_key: None,
            source: RunSource::Api,
            source_ref: None,
            parent: None,
        })
        .await
        .expect("stable room run should succeed");

    let captured = requests
        .lock()
        .expect("request capture should not be poisoned");
    let texts = captured[0]
        .messages
        .iter()
        .map(|message| message.content.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        ["telegram question", "telegram answer", "telegram follow-up"]
    );
    assert!(!texts.contains(&"ordinary message"));
}

#[tokio::test]
async fn idempotency_key_is_propagated_to_runtime_input_metadata() {
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let adapter = Arc::new(CapturingModelAdapter {
        requests: Arc::clone(&requests),
    });
    let (coordinator, agent_id) = coordinator_with_agent(adapter, 2).await;

    coordinator
        .run(AgentRunRequest {
            agent_id,
            content: Content {
                text: "retryable".into(),
                metadata: Some(BTreeMap::from([(
                    "source".into(),
                    DataValue::String("telegram".into()),
                )])),
                attachments: None,
            },
            room: RunRoom::Stable("room-telegram".into()),
            idempotency_key: Some("connector:update:42".into()),
            source: RunSource::Api,
            source_ref: None,
            parent: None,
        })
        .await
        .expect("run should succeed");

    let captured = requests
        .lock()
        .expect("request capture should not be poisoned");
    let metadata = captured[0].messages[0]
        .content
        .metadata
        .as_ref()
        .expect("metadata should be present");
    assert_eq!(
        metadata.get("idempotencyKey"),
        Some(&DataValue::String("connector:update:42".into()))
    );
    assert_eq!(
        metadata.get("source"),
        Some(&DataValue::String("telegram".into()))
    );
}

#[tokio::test]
async fn control_plane_transaction_is_shared_by_coordinator_clones() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let coordinator = AgentRunCoordinator::new(state, Arc::new(Semaphore::new(1)));
    let first = coordinator.control_plane_transaction().await;
    let contender = coordinator.clone();
    let waiting = tokio::spawn(async move {
        let _guard = contender.control_plane_transaction().await;
    });

    assert!(tokio::time::timeout(Duration::from_millis(20), waiting)
        .await
        .is_err());
    drop(first);
}

#[tokio::test]
async fn commit_hook_sees_the_merged_run_before_the_final_snapshot() {
    let path = snapshot_path("commit-order");
    let config = ControlPlaneStoreConfig::Json(path.clone());
    let adapter = Arc::new(CapturingModelAdapter {
        requests: Arc::new(StdMutex::new(Vec::new())),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(adapter)));
    let agent_id = {
        let mut guard = state.write().await;
        guard.set_control_plane_store(Some(config.clone()));
        guard
            .create_agent(test_config("before-commit"))
            .expect("agent should be created")
            .state
            .id
    };
    let coordinator = AgentRunCoordinator::new(state, Arc::new(Semaphore::new(2)));
    let commit_agent_id = agent_id.clone();

    coordinator
        .run_with_commit(request(&agent_id, "commit me"), move |state, outcome| {
            let agent = state
                .get_agent(&commit_agent_id)
                .expect("the canonical agent stays registered");
            let reply = outcome
                .reply_message_id
                .as_deref()
                .expect("a successful run has a reply");
            assert!(agent.messages.iter().any(|message| {
                message.id == reply
                    && message.role == MessageRole::Assistant
                    && message.room_id == outcome.session_id
            }));
            assert_eq!(outcome.status, RunStatus::Completed);
            assert_eq!(
                state.runs.get(&outcome.run_id).map(|run| run.status),
                Some(RunStatus::Completed)
            );
            state
                .update_agent(
                    &commit_agent_id,
                    AgentConfigUpdate {
                        name: Some("after-commit".into()),
                        ..AgentConfigUpdate::default()
                    },
                )
                .expect("commit mutation should succeed");
            Ok(())
        })
        .await
        .expect("run and commit should succeed");

    let persisted = load_control_plane_snapshot(&config)
        .await
        .expect("snapshot should load")
        .expect("snapshot should exist");
    assert_eq!(persisted.agents[0].state.config.name, "after-commit");
    assert_eq!(persisted.runs.len(), 1);
    assert_eq!(persisted.runs[0].status, RunStatus::Completed);
    assert_eq!(persisted.runs[0].source, RunSource::Api);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn rejected_commit_rolls_back_only_the_run_and_keeps_the_running_marker() {
    let path = snapshot_path("commit-failure");
    let config = ControlPlaneStoreConfig::Json(path.clone());
    let adapter = Arc::new(CapturingModelAdapter {
        requests: Arc::new(StdMutex::new(Vec::new())),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(adapter)));
    let agent_id = {
        let mut guard = state.write().await;
        guard.set_control_plane_store(Some(config.clone()));
        guard
            .create_agent(test_config("commit-failure"))
            .expect("agent should be created")
            .state
            .id
    };
    let coordinator = AgentRunCoordinator::new(Arc::clone(&state), Arc::new(Semaphore::new(2)));

    let error = coordinator
        .run_with_commit(request(&agent_id, "fail commit"), |state, outcome| {
            assert!(state.runs.get(&outcome.run_id).is_some());
            Err(ApiError::bad_request("commit rejected"))
        })
        .await
        .expect_err("a rejected commit fails the run");
    assert_eq!(error.message(), "commit rejected");
    {
        let guard = state.read().await;
        let agent = guard
            .get_agent(&agent_id)
            .expect("the canonical runtime stays registered");
        assert!(
            agent.messages.is_empty(),
            "the rejected run's messages are removed"
        );
        assert_eq!(agent.state.status, AgentStatus::Idle);
        let run = guard.runs.for_agent(&agent_id)[0].clone();
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(
            run.error.map(|error| error.code),
            Some("commit_rejected".to_string())
        );
    }
    let persisted = load_control_plane_snapshot(&config)
        .await
        .expect("snapshot should load")
        .expect("running snapshot should exist");
    assert_eq!(persisted.agents[0].state.status, AgentStatus::Running);
    assert_eq!(persisted.runs[0].status, RunStatus::Running);
    let _ = std::fs::remove_file(path);
}

struct ConfigGateModelAdapter {
    names: StdMutex<Vec<String>>,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

#[async_trait]
impl ModelAdapter for ConfigGateModelAdapter {
    fn provider(&self) -> &str {
        "config-gate"
    }

    async fn generate(
        &self,
        config: &AgentConfig,
        _request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        self.names.lock().unwrap().push(config.name.clone());
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        Ok(model_response("done"))
    }
}

struct PanickingModelAdapter;

#[async_trait]
impl ModelAdapter for PanickingModelAdapter {
    fn provider(&self) -> &str {
        "panicking"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        _request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        panic!("model adapter crashed");
    }
}

#[tokio::test]
async fn patch_during_a_run_applies_to_later_runs_only() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let adapter = Arc::new(ConfigGateModelAdapter {
        names: StdMutex::new(Vec::new()),
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let (coordinator, agent_id) = coordinator_with_agent(adapter.clone(), 2).await;
    let running = {
        let coordinator = coordinator.clone();
        let request = request(&agent_id, "first");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    entered.acquire().await.unwrap().forget();

    coordinator
        .state
        .write()
        .await
        .update_agent(
            &agent_id,
            AgentConfigUpdate {
                name: Some("renamed".into()),
                ..AgentConfigUpdate::default()
            },
        )
        .unwrap();
    release.add_permits(1);
    running.await.unwrap().unwrap();

    let config = coordinator
        .state
        .read()
        .await
        .get_agent(&agent_id)
        .unwrap()
        .state
        .config;
    assert_eq!(config.name, "renamed");
    assert_eq!(
        config.system, None,
        "per-run prompts never reach the canonical config"
    );
    assert_eq!(
        config.tools, None,
        "per-run tools never reach the canonical config"
    );
    release.add_permits(1);
    coordinator.run(request(&agent_id, "second")).await.unwrap();
    assert_eq!(
        adapter.names.lock().unwrap().clone(),
        ["operator", "renamed"]
    );
}

#[tokio::test]
async fn every_run_gets_a_ledger_record_with_its_source_room_and_input() {
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;
    let mut stable = request(&agent_id, "stable room");
    stable.room = RunRoom::Stable("direct:ledger".into());
    coordinator.run(stable).await.unwrap();
    coordinator
        .run(request(&agent_id, "generated room"))
        .await
        .unwrap();

    let guard = coordinator.state.read().await;
    let runs = guard.runs.for_agent(&agent_id);
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|run| {
        run.id.starts_with("run_")
            && run.status == RunStatus::Completed
            && run.source == RunSource::Api
            && run.finished_at_ms.is_some()
            && !run.mirrored
    }));
    let stable = runs
        .iter()
        .find(|run| run.input.text == "stable room")
        .unwrap();
    assert_eq!(stable.session_id, "direct:ledger");
    let generated = runs
        .iter()
        .find(|run| run.input.text == "generated room")
        .unwrap();
    assert!(generated.session_id.starts_with("room-"));
    let agent = guard.get_agent(&agent_id).unwrap();
    assert!(
        agent
            .messages
            .iter()
            .any(|message| message.room_id == generated.session_id),
        "a generated room is chosen before the run and used for its messages"
    );
}

#[tokio::test]
async fn a_second_in_flight_run_with_the_same_idempotency_key_is_rejected() {
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::clone(&requests),
        }),
        2,
    )
    .await;
    coordinator
        .state
        .write()
        .await
        .runs
        .insert(RunRecord::running(
            RunStart {
                agent_id: agent_id.clone(),
                session_id: "room-other".into(),
                source: RunSource::Telegram,
                source_ref: None,
                idempotency_key: Some("dup-key".into()),
                text: "in flight".into(),
                model: "gpt-5.4".into(),
                provider: None,
                parent_run_id: None,
            },
            anima_core::primitives::now_millis(),
        ));
    let mut duplicate = request(&agent_id, "same logical work");
    duplicate.idempotency_key = Some("dup-key".into());

    let error = coordinator
        .run(duplicate)
        .await
        .expect_err("one logical unit of work runs once at a time");

    assert_eq!(error.status(), StatusCode::CONFLICT);
    assert_eq!(
        error.message(),
        "A run with this idempotency key is already in progress"
    );
    assert!(requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_crashed_run_never_stays_in_flight() {
    let (coordinator, agent_id) = coordinator_with_agent(Arc::new(PanickingModelAdapter), 2).await;

    let error = coordinator
        .run(request(&agent_id, "crash"))
        .await
        .expect_err("the run task panicked");
    assert_eq!(error.message(), "agent run worker stopped unexpectedly");

    for _ in 0..100 {
        if coordinator.state.read().await.in_flight_runs(&agent_id) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let guard = coordinator.state.read().await;
    assert_eq!(guard.in_flight_runs(&agent_id), 0);
    let run = guard.runs.for_agent(&agent_id)[0].clone();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error.map(|error| error.code),
        Some("run_aborted".to_string())
    );
    assert_ne!(
        guard.get_agent(&agent_id).unwrap().state.status,
        AgentStatus::Running
    );
}

#[tokio::test]
async fn a_run_that_stops_between_its_ledger_insert_and_start_save_never_stays_in_flight() {
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::clone(&requests),
        }),
        2,
    )
    .await;
    // The run-start persist request overflows the revision counter, so the
    // run task panics after recording the run and before its start save.
    coordinator
        .state
        .write()
        .await
        .set_control_plane_revision_for_test(u64::MAX);

    let error = coordinator
        .run(request(&agent_id, "never saved"))
        .await
        .expect_err("the run task panicked");
    assert_eq!(error.message(), "agent run worker stopped unexpectedly");

    for _ in 0..100 {
        if coordinator.state.read().await.in_flight_runs(&agent_id) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let guard = coordinator.state.read().await;
    assert_eq!(guard.in_flight_runs(&agent_id), 0);
    let run = guard.runs.for_agent(&agent_id)[0].clone();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error.map(|error| error.code),
        Some("run_aborted".to_string())
    );
    assert!(requests.lock().unwrap().is_empty(), "the model never ran");
}

struct ToolThenGateModelAdapter {
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

#[async_trait]
impl ModelAdapter for ToolThenGateModelAdapter {
    fn provider(&self) -> &str {
        "tool-then-gate"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        if !request
            .messages
            .iter()
            .any(|message| message.role == MessageRole::Tool)
        {
            let mut response = model_response("Calculating");
            response.stop_reason = ModelStopReason::ToolCall;
            response.tool_calls = Some(vec![anima_core::ToolCall {
                id: "calculate-1".into(),
                name: "calculate".into(),
                args: BTreeMap::from([("expression".into(), DataValue::String("1 + 2".into()))]),
            }]);
            return Ok(response);
        }
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        Ok(model_response("The answer is 3"))
    }
}

#[tokio::test]
async fn a_run_interrupted_after_starting_a_tool_keeps_that_tool_across_a_restart() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(Arc::new(
        ToolThenGateModelAdapter {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        },
    ))));
    let mut config = test_config("calculator");
    config.tools = Some(
        crate::tools::ToolRegistry::new()
            .resolve_descriptors(["calculate"])
            .unwrap(),
    );
    let agent_id = state.write().await.create_agent(config).unwrap().state.id;
    let coordinator = AgentRunCoordinator::new(Arc::clone(&state), Arc::new(Semaphore::new(2)));
    let running = {
        let coordinator = coordinator.clone();
        let request = request(&agent_id, "what is 1 + 2?");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    entered.acquire().await.unwrap().forget();

    // Any save taken now carries the tool the run already started, and a
    // restart then interrupts the run.
    let snapshot = state.read().await.control_plane_snapshot();
    let restored = RunLedger::restored(
        snapshot.runs,
        &HashSet::from([agent_id.clone()]),
        anima_core::primitives::now_millis(),
    );
    let interrupted = restored.for_agent(&agent_id)[0].clone();
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert_eq!(
        interrupted.error.map(|error| error.code),
        Some("restart_during_run".to_string())
    );
    assert_eq!(interrupted.tools_started, ["calculate"]);

    release.add_permits(1);
    running.await.unwrap().unwrap();
    assert_eq!(
        state.read().await.runs.for_agent(&agent_id)[0].tools_started,
        ["calculate"],
        "the commit-time fill agrees"
    );
}

#[tokio::test]
async fn a_failed_start_save_leaves_no_ledger_record() {
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::clone(&requests),
        }),
        2,
    )
    .await;
    let gate = coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);

    let error = coordinator
        .run(request(&agent_id, "unsaved"))
        .await
        .expect_err("the run-start save failed");
    assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);

    tokio::time::sleep(Duration::from_millis(20)).await;
    let guard = coordinator.state.read().await;
    assert!(guard.runs.for_agent(&agent_id).is_empty());
    assert_eq!(guard.in_flight_runs(&agent_id), 0);
    assert!(requests.lock().unwrap().is_empty(), "the model never ran");
}

#[tokio::test]
async fn a_failed_final_save_fails_the_run_and_reverts_only_its_transcript() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(GateModelAdapter {
            calls: AtomicUsize::new(0),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
        4,
    )
    .await;
    // A turn committed in another room, which the failed runs must keep.
    release.add_permits(1);
    coordinator
        .run(room_request(&agent_id, "room-kept", "kept turn"))
        .await
        .expect("an ordinary run commits");
    entered.acquire().await.unwrap().forget();
    let committed = coordinator
        .state
        .read()
        .await
        .get_agent(&agent_id)
        .unwrap()
        .messages;
    assert_eq!(committed.len(), 2);

    // The legacy route's run, then a job's ticketed run; each fails only its
    // final save.
    let legacy = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "room-legacy", "legacy turn");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    fail_the_next_final_save(&coordinator, &entered, &release).await;
    let legacy_error = legacy.await.unwrap().expect_err("the final save failed");

    let source_rollbacks = Arc::new(AtomicUsize::new(0));
    let ticket = coordinator
        .try_ticket(&agent_id, "job:job-1")
        .await
        .expect("the job's room, a slot, and a permit are free");
    let job = {
        let coordinator = coordinator.clone();
        let source_rollbacks = Arc::clone(&source_rollbacks);
        let request = AgentRunRequest {
            idempotency_key: Some("job:job-1:attempt:1".into()),
            source: RunSource::Job,
            source_ref: Some("job-1:1".into()),
            ..room_request(&agent_id, "job:job-1", "job turn")
        };
        tokio::spawn(async move {
            coordinator
                .run_ticketed_with_commit_and_rollback(
                    request,
                    ticket,
                    |_, _| Ok(()),
                    move |_| {
                        source_rollbacks.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
        })
    };
    fail_the_next_final_save(&coordinator, &entered, &release).await;
    let job_error = job.await.unwrap().expect_err("the final save failed");

    for error in [&legacy_error, &job_error] {
        assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    assert_eq!(
        source_rollbacks.load(Ordering::SeqCst),
        1,
        "the job's own rollback ran"
    );
    let guard = coordinator.state.read().await;
    assert_eq!(
        guard.get_agent(&agent_id).unwrap().messages,
        committed,
        "only the failed runs' messages are removed"
    );
    for (room, source) in [
        ("room-legacy", RunSource::Api),
        ("job:job-1", RunSource::Job),
    ] {
        let run = guard
            .runs
            .for_agent(&agent_id)
            .into_iter()
            .find(|run| run.session_id == room)
            .expect("the failed run keeps its ledger record");
        assert_eq!(run.source, source);
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(
            run.error.as_ref().map(|error| error.code.as_str()),
            Some("commit_failed")
        );
    }
    drop(guard);
    assert_eq!(coordinator.lock_counts(), (0, 0));
}

/// Waits for the run parked in the gate model, makes the next control-plane
/// save (its final save) fail, then releases the run.
async fn fail_the_next_final_save(
    coordinator: &AgentRunCoordinator,
    entered: &Semaphore,
    release: &Semaphore,
) {
    entered.acquire().await.unwrap().forget();
    let gate = coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    release.add_permits(1);
}

fn request(agent_id: &str, text: &str) -> AgentRunRequest {
    AgentRunRequest {
        agent_id: agent_id.to_string(),
        content: Content {
            text: text.to_string(),
            ..Content::default()
        },
        room: RunRoom::Generated,
        idempotency_key: None,
        source: RunSource::Api,
        source_ref: None,
        parent: None,
    }
}

fn room_request(agent_id: &str, room_id: &str, text: &str) -> AgentRunRequest {
    AgentRunRequest {
        room: RunRoom::Stable(room_id.into()),
        ..request(agent_id, text)
    }
}

async fn coordinator_with_agent(
    adapter: Arc<dyn ModelAdapter>,
    permits: usize,
) -> (AgentRunCoordinator, String) {
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(adapter)));
    let agent_id = state
        .write()
        .await
        .create_agent(test_config("operator"))
        .expect("agent should be created")
        .state
        .id;
    (
        AgentRunCoordinator::new(state, Arc::new(Semaphore::new(permits))),
        agent_id,
    )
}

fn message(agent_id: &str, room_id: &str, text: &str, role: MessageRole) -> Message {
    Message {
        id: format!("message-{room_id}-{text}"),
        agent_id: agent_id.to_string(),
        room_id: room_id.to_string(),
        content: Content {
            text: text.to_string(),
            ..Content::default()
        },
        role,
        created_at_ms: 1,
    }
}

fn model_response(text: &str) -> ModelGenerateResponse {
    ModelGenerateResponse {
        content: Content {
            text: text.to_string(),
            ..Content::default()
        },
        tool_calls: None,
        usage: TokenUsage::default(),
        stop_reason: ModelStopReason::End,
    }
}

fn test_config(name: &str) -> AgentConfig {
    AgentConfig {
        name: name.into(),
        model: "gpt-5.4".into(),
        bio: None,
        lore: None,
        knowledge: None,
        topics: None,
        adjectives: None,
        style: None,
        provider: Some("openai".into()),
        system: None,
        tools: None,
        plugins: None,
        settings: Some(AgentSettings::default()),
        fallbacks: Vec::new(),
    }
}

fn snapshot_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "anima-agent-runs-{label}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ))
}

#[tokio::test]
async fn only_durable_commits_reach_the_history_outbox() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(GateModelAdapter {
            calls: AtomicUsize::new(0),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
        4,
    )
    .await;
    release.add_permits(1);
    coordinator
        .run(room_request(&agent_id, "room-kept", "kept turn"))
        .await
        .expect("an ordinary run commits");
    entered.acquire().await.unwrap().forget();
    let history = coordinator.state.read().await.history.clone();
    assert_eq!(history.pending_count(), 2, "the committed turn is queued");

    let failed = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "room-lost", "lost turn");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    fail_the_next_final_save(&coordinator, &entered, &release).await;
    failed.await.unwrap().expect_err("the final save failed");

    assert_eq!(
        history.pending_count(),
        2,
        "a commit whose save failed never reaches the history store"
    );
}

/// Flushes while a run's final save is in flight and then fails: the
/// flush waits for the rollback, then mirrors the run as failed and none
/// of its messages. `reconciled` flushes once first, so the flush under
/// test skips the reconcile and reaches the runs read directly.
async fn assert_a_flush_during_a_failing_final_save_mirrors_only_saved_state(reconciled: bool) {
    use crate::history::{HistoryService, HistoryStore, MemoryHistoryStore, MessagePageQuery};

    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(GateModelAdapter {
            calls: AtomicUsize::new(0),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
        4,
    )
    .await;
    let store = Arc::new(MemoryHistoryStore::new());
    let history = HistoryService::new(store.clone());
    coordinator
        .state
        .write()
        .await
        .set_history(Arc::clone(&history));
    let transactions = coordinator.control_plane_transactions();
    if reconciled {
        history
            .flush_once(
                &coordinator.state,
                &transactions,
                anima_core::primitives::now_millis(),
            )
            .await
            .unwrap();
        assert!(history.reconciled());
    }
    let run = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "room-lost", "lost turn");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    entered.acquire().await.unwrap().forget();
    let gate = coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    release.add_permits(1);
    // The commit is merged in memory and its final save is in flight.
    gate.entered.acquire().await.unwrap().forget();

    let flush = {
        let history = Arc::clone(&history);
        let state = Arc::clone(&coordinator.state);
        let transactions = Arc::clone(&transactions);
        tokio::spawn(async move {
            history
                .flush_once(&state, &transactions, anima_core::primitives::now_millis())
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !flush.is_finished(),
        "a flush waits until the commit is saved or rolled back"
    );
    gate.release.add_permits(1);
    run.await.unwrap().expect_err("the final save failed");
    let report = flush.await.unwrap().expect("the flush succeeds");

    let record = coordinator.state.read().await.runs.for_agent(&agent_id)[0].clone();
    assert_eq!(record.status, RunStatus::Failed);
    let stored = store
        .get_run(&record.id)
        .await
        .unwrap()
        .expect("the rolled-back run is mirrored");
    assert_eq!(stored.status, RunStatus::Failed);
    assert_eq!(report.messages, 0);
    assert!(
        store
            .page_messages(&MessagePageQuery {
                agent_id: agent_id.clone(),
                session_id: "room-lost".into(),
                before: None,
                limit: 10,
                include_hidden: true,
            })
            .await
            .unwrap()
            .is_empty(),
        "no phantom rows for messages the rollback removed"
    );
}

#[tokio::test]
async fn a_flush_during_a_failing_final_save_mirrors_the_rolled_back_run_without_its_messages() {
    assert_a_flush_during_a_failing_final_save_mirrors_only_saved_state(true).await;
}

#[tokio::test]
async fn a_first_flush_during_a_failing_final_save_reconciles_only_saved_messages() {
    assert_a_flush_during_a_failing_final_save_mirrors_only_saved_state(false).await;
}

#[tokio::test]
async fn a_run_saves_its_session_with_the_run_start_and_advances_it_at_commit() {
    use crate::sessions::{SessionKind, SessionOrigin, TitleSource};

    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(GateModelAdapter {
            calls: AtomicUsize::new(0),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
        4,
    )
    .await;
    let path = snapshot_path("session-start");
    let store = ControlPlaneStoreConfig::Json(path.clone());
    coordinator
        .state
        .write()
        .await
        .set_control_plane_store(Some(store.clone()));
    let running = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "chat:plan", "Plan the offsite\nsoon");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    entered.acquire().await.unwrap().forget();

    let saved = load_control_plane_snapshot(&store).await.unwrap().unwrap();
    let started = saved
        .sessions
        .iter()
        .find(|session| session.id == "chat:plan")
        .expect("the run-start save carries the new session");
    assert_eq!(
        (started.kind, started.origin),
        (SessionKind::Chat, SessionOrigin::Web)
    );
    assert_eq!(started.title, "Plan the offsite");
    assert_eq!(started.title_source, TitleSource::FirstMessage);
    assert_eq!(saved.runs[0].session_id, "chat:plan");

    release.add_permits(1);
    running.await.unwrap().unwrap();
    let guard = coordinator.state.read().await;
    let session = guard.sessions.get(&agent_id, "chat:plan").unwrap();
    let messages = guard.get_agent(&agent_id).unwrap().messages;
    assert_eq!(
        session.last_activity_at_ms,
        messages
            .iter()
            .map(|message| message.created_at_ms)
            .max()
            .unwrap()
    );
    assert_eq!(
        session.last_read_at_ms,
        Some(messages[0].created_at_ms),
        "the owner's own message is read; the reply is not"
    );
    drop(guard);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_failed_start_save_leaves_no_session_record() {
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;
    let gate = coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);

    coordinator
        .run(room_request(&agent_id, "chat:unsaved", "unsaved"))
        .await
        .expect_err("the run-start save failed");

    assert!(coordinator
        .state
        .read()
        .await
        .sessions
        .get(&agent_id, "chat:unsaved")
        .is_none());
}

/// Pins the `if session_created` guard at `agent_runs.rs`'s run-start
/// failure branch: a second run's failed start save in a room that
/// already has a session must leave that pre-existing record untouched.
#[tokio::test]
async fn a_failed_start_save_does_not_remove_a_session_it_did_not_create() {
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;

    coordinator
        .run(room_request(&agent_id, "chat:reused", "first turn"))
        .await
        .expect("the first run commits and creates the session");
    let original = coordinator
        .state
        .read()
        .await
        .sessions
        .get(&agent_id, "chat:reused")
        .cloned()
        .expect("the session exists after the first run");

    let gate = coordinator
        .state
        .write()
        .await
        .install_test_control_plane_save_gate(true);
    gate.release.add_permits(1);
    coordinator
        .run(room_request(&agent_id, "chat:reused", "second turn"))
        .await
        .expect_err("the second run's start save failed");

    let after = coordinator
        .state
        .read()
        .await
        .sessions
        .get(&agent_id, "chat:reused")
        .cloned()
        .expect("a session this run did not create must survive its failed start save");
    assert_eq!(
        after, original,
        "a failed start save must not touch a session it found rather than created"
    );
}

#[tokio::test]
async fn a_rolled_back_commit_restores_the_session_it_advanced() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(GateModelAdapter {
            calls: AtomicUsize::new(0),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
        4,
    )
    .await;
    let failed = {
        let coordinator = coordinator.clone();
        let request = room_request(&agent_id, "chat:lost", "lost turn");
        tokio::spawn(async move { coordinator.run(request).await })
    };
    fail_the_next_final_save(&coordinator, &entered, &release).await;
    failed.await.unwrap().expect_err("the final save failed");

    let guard = coordinator.state.read().await;
    let session = guard
        .sessions
        .get(&agent_id, "chat:lost")
        .expect("the session was saved with the run start");
    assert_eq!(
        session.last_activity_at_ms, session.created_at_ms,
        "the rolled-back turn no longer counts as activity"
    );
    assert_eq!(session.last_read_at_ms, None);
}

#[tokio::test]
async fn a_delegated_run_links_its_parent_run_and_session() {
    use crate::sessions::{SessionKind, SessionOrigin};

    let adapter = Arc::new(TeamModelAdapter {
        target: StdMutex::new(String::new()),
        configs: StdMutex::new(vec![]),
        systems: StdMutex::new(vec![]),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let mut manager = test_config("Manager");
    manager.tools = Some(vec![crate::tools::ToolRegistry::new()
        .descriptor("send_message")
        .unwrap()]);
    manager
        .settings
        .as_mut()
        .unwrap()
        .additional
        .insert("workspaceRole".into(), DataValue::String("lead".into()));
    let manager = state.write().await.create_agent(manager).unwrap().state;
    let mut worker_config = test_config("Alice");
    worker_config.tools = manager.config.tools.clone();
    let worker = state
        .write()
        .await
        .create_agent(worker_config)
        .unwrap()
        .state;
    *adapter.target.lock().unwrap() = worker.id.clone();
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));

    coordinator
        .run(request(&manager.id, "Ask Alice to draft a plan"))
        .await
        .unwrap();

    let guard = state.read().await;
    let manager_run = guard.runs.for_agent(&manager.id)[0].clone();
    let worker_run = guard.runs.for_agent(&worker.id)[0].clone();
    assert_eq!(manager_run.parent_run_id, None);
    assert_eq!(worker_run.source, RunSource::Delegation);
    assert_eq!(
        worker_run.parent_run_id.as_deref(),
        Some(manager_run.id.as_str())
    );
    let helper = guard
        .sessions
        .get(&worker.id, &worker_run.session_id)
        .expect("the delegated room is a session");
    assert_eq!(
        (helper.kind, helper.origin),
        (SessionKind::Helper, SessionOrigin::Delegation)
    );
    assert_eq!(
        helper.parent_run_id.as_deref(),
        Some(manager_run.id.as_str())
    );
    assert_eq!(
        helper.parent_session_id.as_deref(),
        Some(manager_run.session_id.as_str())
    );
    assert_eq!(helper.parent_agent_id.as_deref(), Some(manager.id.as_str()));
    assert_eq!(helper.title, "Draft a content plan");
    assert_eq!(
        helper.last_read_at_ms, None,
        "a delegated run is not the owner's own turn"
    );
    let chat = guard
        .sessions
        .get(&manager.id, &manager_run.session_id)
        .expect("the manager's generated room is a session too");
    assert_eq!(chat.kind, SessionKind::Chat);
}

#[tokio::test]
async fn spawned_helpers_record_the_companion_run_that_started_them() {
    use crate::sessions::{SessionKind, SessionOrigin};

    let (coordinator, _) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        4,
    )
    .await;
    let lead = helper_lead(&coordinator).await;
    let link = crate::runs::RunLink {
        run_id: "run_companion".into(),
        session_id: "chat:plan".into(),
        agent_id: lead.id.clone(),
    };

    let text = coordinator
        .spawn_helper(
            lead.id.clone(),
            "Scout".into(),
            "Find three sources".into(),
            Some(link),
        )
        .await
        .expect("the helper runs");

    let helper_id = serde_json::from_str::<serde_json::Value>(&text).unwrap()["agentId"]
        .as_str()
        .unwrap()
        .to_string();
    let guard = coordinator.state.read().await;
    let run = guard.runs.for_agent(&helper_id)[0].clone();
    assert_eq!(run.parent_run_id.as_deref(), Some("run_companion"));
    let session = guard
        .sessions
        .get(&helper_id, &run.session_id)
        .expect("the helper room is a session");
    assert_eq!(
        (session.kind, session.origin),
        (SessionKind::Helper, SessionOrigin::Delegation)
    );
    assert_eq!(session.parent_session_id.as_deref(), Some("chat:plan"));
    assert_eq!(session.parent_agent_id.as_deref(), Some(lead.id.as_str()));
    assert_eq!(session.title, "Find three sources");
}

#[tokio::test]
async fn a_peer_request_is_a_helper_session_of_the_recipient() {
    use crate::sessions::{SessionKind, SessionOrigin};

    let (coordinator, sender) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        4,
    )
    .await;
    let recipient = coordinator
        .state
        .write()
        .await
        .create_agent(test_config("Recipient"))
        .unwrap()
        .state
        .id;

    coordinator
        .send_peer(
            sender.clone(),
            recipient.clone(),
            "Check this".into(),
            anima_core::AgentCommunicationRoute::start(sender.clone()),
            None,
        )
        .await
        .expect("the peer request runs");

    let guard = coordinator.state.read().await;
    let session = guard
        .sessions
        .get(&recipient, &format!("peer:{sender}:{recipient}"))
        .expect("the peer room is a session");
    assert_eq!(
        (session.kind, session.origin),
        (SessionKind::Helper, SessionOrigin::Peer)
    );
    assert_eq!(session.parent_agent_id.as_deref(), Some(sender.as_str()));
    assert_eq!(session.parent_run_id, None);
    assert_eq!(session.title, "Messages from operator");
    assert_eq!(
        session.last_read_at_ms, None,
        "a peer request is not the owner's own turn"
    );
}

/// The `is_owner_web_turn` branch of `commit_run`'s `owner_authored`
/// (`run_commit.rs`): a Telegram owner turn sent from the web console
/// runs with `source: telegram`, not `api`/`web`, but its message
/// metadata marks it as the owner's own turn (mirrors
/// `connectors::runtime::send_from_owner_owned`).
#[tokio::test]
async fn a_telegram_owner_turn_sent_from_the_web_marks_the_session_read() {
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;

    coordinator
        .run(AgentRunRequest {
            agent_id: agent_id.clone(),
            content: Content {
                text: "sent from the web console".into(),
                metadata: Some(BTreeMap::from([
                    ("source".into(), DataValue::String("telegramThread".into())),
                    ("connectorId".into(), DataValue::String("conn-1".into())),
                ])),
                attachments: None,
            },
            room: RunRoom::Stable("telegram:conn-1".into()),
            idempotency_key: None,
            source: RunSource::Telegram,
            source_ref: Some("conn-1".into()),
            parent: None,
        })
        .await
        .expect("the owner's web-sent telegram turn runs");

    let guard = coordinator.state.read().await;
    let session = guard
        .sessions
        .get(&agent_id, "telegram:conn-1")
        .expect("the telegram room is a session");
    let messages = guard.get_agent(&agent_id).unwrap().messages;
    assert_eq!(
        session.last_read_at_ms,
        Some(messages[0].created_at_ms),
        "a telegram-thread turn sent from the web is the owner's own turn even though the run's source is telegram, not api or web"
    );
}

/// A schedule-triggered run's source is neither `api` nor `web`, and its
/// check-in prompt carries no `telegramThread` metadata, so it is not the
/// owner's own turn either.
#[tokio::test]
async fn a_schedule_triggered_run_does_not_mark_its_session_owner_read() {
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;

    coordinator
        .run(AgentRunRequest {
            agent_id: agent_id.clone(),
            content: Content {
                text: "checkin: how is it going?".into(),
                ..Content::default()
            },
            room: RunRoom::Stable("schedule:sched-1".into()),
            idempotency_key: None,
            source: RunSource::Schedule,
            source_ref: Some("sched-1".into()),
            parent: None,
        })
        .await
        .expect("the scheduled run commits");

    let guard = coordinator.state.read().await;
    let session = guard
        .sessions
        .get(&agent_id, "schedule:sched-1")
        .expect("the schedule room is a session");
    assert_eq!(
        session.last_read_at_ms, None,
        "a scheduled check-in is not the owner's own turn"
    );
}

/// Controller ruling (M2 pre-flight audit): a calendar write's own
/// confirmation follow-up (`RunRoom::Generated`, source `api`, `sourceRef
/// = calendar-write:<id>`) creates a session titled `system` from the
/// write's own summary, and that follow-up is not the owner's own turn,
/// so the session stays unread.
#[tokio::test]
async fn a_calendar_write_followup_gets_a_system_titled_session_that_is_not_owner_read() {
    use crate::sessions::TitleSource;

    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;
    let reference = "calendar-write:evt-1".to_string();

    coordinator
        .run_system_followup(AgentRunRequest {
            agent_id: agent_id.clone(),
            content: Content {
                text: "Calendar change confirmed and applied: Team sync. Continue the conversation accordingly.".into(),
                metadata: Some(BTreeMap::from([(
                    "calendarSummary".into(),
                    DataValue::String("Team sync".into()),
                )])),
                attachments: None,
            },
            room: RunRoom::Generated,
            idempotency_key: Some(reference.clone()),
            source: RunSource::Api,
            source_ref: Some(reference),
            parent: None,
        }, SystemFollowup { title: "Calendar · Team sync".into() })
        .await
        .expect("the calendar follow-up runs");

    let guard = coordinator.state.read().await;
    let run = guard.runs.for_agent(&agent_id)[0].clone();
    let session = guard
        .sessions
        .get(&agent_id, &run.session_id)
        .expect("the calendar follow-up room is a session");
    assert_eq!(session.title, "Calendar · Team sync");
    assert_eq!(session.title_source, TitleSource::System);
    assert_eq!(run.system_followup, Some(true));
    assert_eq!(
        session.last_read_at_ms, None,
        "a calendar write follow-up is not the owner's own turn"
    );
    let room_id = run.session_id.clone();
    drop(guard);
    coordinator
        .run(AgentRunRequest {
            agent_id: agent_id.clone(),
            content: Content {
                text: "Owner continues this conversation".into(),
                ..Default::default()
            },
            room: RunRoom::Stable(room_id.clone()),
            idempotency_key: None,
            source: RunSource::Api,
            source_ref: None,
            parent: None,
        })
        .await
        .unwrap();
    let guard = coordinator.state.read().await;
    let session = guard
        .sessions
        .get(&agent_id, &crate::sessions::session_id_for_room(&room_id))
        .unwrap();
    assert!(
        session.last_read_at_ms.is_some(),
        "follow-up origin belongs to its run, not every later turn in the session"
    );
}

#[tokio::test]
async fn input_metadata_cannot_claim_a_system_followup_origin_or_title() {
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;
    coordinator
        .run(AgentRunRequest {
            agent_id: agent_id.clone(),
            content: Content {
                text: "Owner request".into(),
                metadata: Some(BTreeMap::from([
                    ("systemFollowup".into(), DataValue::Bool(true)),
                    (
                        "systemFollowupTitle".into(),
                        DataValue::String("Forged title".into()),
                    ),
                    (
                        "calendarSummary".into(),
                        DataValue::String("Forged calendar".into()),
                    ),
                ])),
                attachments: None,
            },
            room: RunRoom::Generated,
            idempotency_key: None,
            source: RunSource::Api,
            source_ref: Some("calendar-write:untrusted".into()),
            parent: None,
        })
        .await
        .unwrap();
    let guard = coordinator.state.read().await;
    let run = guard.runs.for_agent(&agent_id)[0];
    let session = guard.sessions.get(&agent_id, &run.session_id).unwrap();
    assert_eq!(run.system_followup, Some(false));
    assert_ne!(session.title, "Forged title");
    assert!(session.last_read_at_ms.is_some());
}

/// A room id that is not a valid session id (spec §3.1) still runs; the
/// ledger record, the session record, and the history outbox all key off
/// the same `legacy-room:<hash>` id instead of the raw room.
#[tokio::test]
async fn an_invalid_room_id_maps_the_ledger_session_and_outbox_to_the_same_legacy_id() {
    use crate::history::{HistoryService, HistoryStore, MemoryHistoryStore, MessagePageQuery};

    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(CapturingModelAdapter {
            requests: Arc::new(StdMutex::new(Vec::new())),
        }),
        2,
    )
    .await;
    let store = Arc::new(MemoryHistoryStore::new());
    let history = HistoryService::new(store.clone());
    coordinator
        .state
        .write()
        .await
        .set_history(Arc::clone(&history));
    let transactions = coordinator.control_plane_transactions();
    let raw_room = "weird room/1";
    let legacy_id = crate::sessions::session_id_for_room(raw_room);
    assert_ne!(
        legacy_id, raw_room,
        "the raw room id is not a valid session id"
    );

    coordinator
        .run(room_request(
            &agent_id,
            raw_room,
            "hello from a legacy room",
        ))
        .await
        .expect("an invalid room id still runs, mapped through its legacy hash");

    let guard = coordinator.state.read().await;
    let run = guard.runs.for_agent(&agent_id)[0].clone();
    assert_eq!(
        run.session_id, legacy_id,
        "the ledger record uses the legacy session id"
    );
    let session = guard
        .sessions
        .get(&agent_id, &legacy_id)
        .expect("the session is keyed by the legacy id");
    assert_eq!(
        session.room_id(),
        raw_room,
        "the session still remembers the real transcript room"
    );
    drop(guard);

    history
        .flush_once(
            &coordinator.state,
            &transactions,
            anima_core::primitives::now_millis(),
        )
        .await
        .expect("the flush succeeds");
    let page = store
        .page_messages(&MessagePageQuery {
            agent_id: agent_id.clone(),
            session_id: legacy_id,
            before: None,
            limit: 10,
            include_hidden: true,
        })
        .await
        .expect("the store can be queried");
    assert!(
        !page.is_empty(),
        "the outbox queued the turn under the legacy session id"
    );
}

/// `tools/team.rs::spawn_helper` passes `context.run_link.clone()`
/// through to `AgentRunCoordinator::spawn_helper`; this drives it through
/// the actual tool call (unlike `spawned_helpers_record_the_companion_run_that_started_them`,
/// which calls the coordinator method directly), so the helper session's
/// parent fields must match the companion's own run and session, not
/// just its `parentAgentId` config fallback.
#[tokio::test]
async fn spawn_helper_through_the_tool_path_links_the_helper_session_to_the_companion_run() {
    let adapter = Arc::new(HelperModelAdapter {
        configs: StdMutex::new(vec![]),
    });
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(
        adapter.clone(),
    )));
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(3)));
    let lead = helper_lead(&coordinator).await;

    coordinator
        .run(request(&lead.id, "Ask a helper to check facts"))
        .await
        .expect("the companion's run, including its spawn_helper tool call, commits");

    let guard = state.read().await;
    let lead_run = guard.runs.for_agent(&lead.id)[0].clone();
    let helper = guard
        .list_agents()
        .into_iter()
        .find(|agent| agent.state.id != lead.id)
        .expect("a helper was created");
    let helper_run = guard.runs.for_agent(&helper.state.id)[0].clone();
    assert_eq!(
        helper_run.parent_run_id.as_deref(),
        Some(lead_run.id.as_str()),
        "the run-link passed through the spawn_helper tool call"
    );
    let session = guard
        .sessions
        .get(&helper.state.id, &helper_run.session_id)
        .expect("the helper room is a session");
    assert_eq!(session.parent_run_id.as_deref(), Some(lead_run.id.as_str()));
    assert_eq!(
        session.parent_session_id.as_deref(),
        Some(lead_run.session_id.as_str())
    );
    assert_eq!(session.parent_agent_id.as_deref(), Some(lead.id.as_str()));
}

/// `tools/team.rs::send_message` also passes `context.run_link.clone()`
/// through; this existing end-to-end test already drives a `send_message`
/// tool call (Alice asking Bob for a review), so the assertion is cheap
/// to add here rather than duplicating the whole fixture.
#[tokio::test]
async fn send_message_through_the_tool_path_links_the_peer_session_to_the_calling_run() {
    let state = Arc::new(RwLock::new(DaemonState::with_model_adapter(Arc::new(
        PeerModelAdapter,
    ))));
    let alice = state
        .write()
        .await
        .create_agent(test_config("Alice"))
        .unwrap()
        .state;
    state
        .write()
        .await
        .create_agent(test_config("Bob"))
        .unwrap();
    let coordinator = AgentRunCoordinator::new(state.clone(), Arc::new(Semaphore::new(4)));
    let mut direct = request(&alice.id, "Ask Bob for a review");
    direct.room = RunRoom::Stable(format!("direct:{}", alice.id));

    coordinator
        .run(direct)
        .await
        .expect("alice's run, including its send_message tool call, commits");

    let guard = state.read().await;
    let alice_run = guard.runs.for_agent(&alice.id)[0].clone();
    let bob = guard
        .list_agents()
        .into_iter()
        .find(|agent| agent.state.name == "Bob")
        .expect("bob exists");
    let bob_run = guard.runs.for_agent(&bob.state.id)[0].clone();
    assert_eq!(
        bob_run.parent_run_id.as_deref(),
        Some(alice_run.id.as_str()),
        "the run-link passed through the send_message tool call"
    );
    let session = guard
        .sessions
        .get(&bob.state.id, &bob_run.session_id)
        .expect("the peer room is a session");
    assert_eq!(
        session.parent_run_id.as_deref(),
        Some(alice_run.id.as_str())
    );
    assert_eq!(session.parent_agent_id.as_deref(), Some(alice.id.as_str()));
}

struct CheckinModel;

#[async_trait]
impl ModelAdapter for CheckinModel {
    fn provider(&self) -> &str {
        "checkin"
    }

    async fn generate(
        &self,
        _config: &AgentConfig,
        request: &ModelGenerateRequest,
    ) -> Result<ModelGenerateResponse, String> {
        let input = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::User)
            .map(|message| message.content.text.clone())
            .unwrap_or_default();
        Ok(model_response(if input.contains("quiet") {
            "CHECKIN_OK"
        } else {
            "Two tasks are overdue"
        }))
    }
}

#[tokio::test]
async fn a_silent_checkin_stores_no_task_result_memory_or_reflection() {
    let (coordinator, agent_id) = coordinator_with_agent(Arc::new(CheckinModel), 4).await;
    let checkin = |text: &str| AgentRunRequest {
        content: Content {
            text: crate::schedules::wrap_checkin_prompt(text),
            attachments: None,
            metadata: Some(BTreeMap::from([
                ("kind".to_string(), DataValue::String("checkin".into())),
                ("id".to_string(), DataValue::String("schedule-1".into())),
            ])),
        },
        source: RunSource::Schedule,
        ..room_request(&agent_id, "schedule:schedule-1", "unused")
    };
    let memory = coordinator.state.read().await.memory_handle();

    coordinator.run(checkin("quiet check")).await.unwrap();
    assert_eq!(
        memory.read().await.size(),
        0,
        "a silent check-in leaves no memory"
    );

    coordinator.run(checkin("loud check")).await.unwrap();
    assert!(
        memory.read().await.size() >= 1,
        "a check-in that spoke is remembered as before"
    );
}

#[tokio::test]
async fn a_completed_chat_run_writes_no_reflection_or_derived_memory() {
    let (coordinator, agent_id) = coordinator_with_agent(Arc::new(CheckinModel), 4).await;
    let mut run = room_request(&agent_id, "room-1", "I prefer terse release summaries");
    run.content.metadata = Some(BTreeMap::from([
        ("userId".to_string(), DataValue::String("user-42".into())),
        ("userName".to_string(), DataValue::String("Leo".into())),
    ]));
    let memory = coordinator.state.read().await.memory_handle();

    coordinator.run(run).await.unwrap();

    let guard = memory.read().await;
    let stored = guard.get_recent(anima_memory::RecentMemoryOptions {
        limit: Some(50),
        ..Default::default()
    });
    assert_eq!(stored.len(), 1, "exactly the task result is stored");
    assert!(
        stored
            .iter()
            .all(|memory| memory.memory_type == anima_memory::MemoryType::TaskResult),
        "only the task result is stored, found {:?}",
        stored
            .iter()
            .map(|memory| (&memory.memory_type, &memory.content))
            .collect::<Vec<_>>()
    );
    assert!(guard
        .list_agent_relationships(anima_memory::AgentRelationshipOptions::default())
        .is_empty());
    assert!(guard
        .list_temporal_relationships(anima_memory::TemporalRelationshipOptions {
            include_inactive: true,
            ..Default::default()
        })
        .is_empty());
}

/// A side post (a commit hook's message in another session, such as an
/// automation's card in the main chat) stands or falls with its run's commit.
#[tokio::test]
async fn a_side_post_is_saved_with_its_commit_and_removed_without_it() {
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let (coordinator, agent_id) = coordinator_with_agent(
        Arc::new(GateModelAdapter {
            calls: AtomicUsize::new(0),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
        4,
    )
    .await;
    super::test_support::add_chat(&coordinator, &agent_id, "chat:target").await;
    let post = |agent_id: &str, id: &str| Message {
        id: id.into(),
        agent_id: agent_id.into(),
        room_id: "chat:target".into(),
        content: Content {
            text: "posted".into(),
            ..Content::default()
        },
        role: MessageRole::Assistant,
        created_at_ms: anima_core::primitives::now_millis(),
    };
    let posted = |coordinator: &AgentRunCoordinator| {
        let state = Arc::clone(&coordinator.state);
        let agent_id = agent_id.clone();
        async move {
            let guard = state.read().await;
            assert!(guard.side_posts.is_empty(), "every commit drains its posts");
            guard
                .get_agent(&agent_id)
                .unwrap()
                .messages
                .into_iter()
                .filter(|message| message.room_id == "chat:target")
                .map(|message| message.id)
                .collect::<Vec<_>>()
        }
    };

    // A hook that stages a post and then refuses: the post goes too.
    release.add_permits(1);
    let staging = agent_id.clone();
    let refused = coordinator
        .run_with_commit_waiting(
            room_request(&agent_id, "room-a", "one"),
            move |state, _| {
                assert!(state.stage_side_post(&staging, "chat:target", post(&staging, "p1")));
                Err(ApiError::conflict("refused"))
            },
            |_| Ok(()),
        )
        .await;
    entered.acquire().await.unwrap().forget();
    assert!(refused.is_err());
    assert!(posted(&coordinator).await.is_empty());

    // A commit whose final save fails: the post goes too.
    let failing = {
        let coordinator = coordinator.clone();
        let staging = agent_id.clone();
        let request = room_request(&agent_id, "room-b", "two");
        tokio::spawn(async move {
            coordinator
                .run_with_commit_waiting(
                    request,
                    move |state, _| {
                        state.stage_side_post(&staging, "chat:target", post(&staging, "p2"));
                        Ok(())
                    },
                    |_| Ok(()),
                )
                .await
        })
    };
    fail_the_next_final_save(&coordinator, &entered, &release).await;
    assert!(failing.await.unwrap().is_err());
    assert!(posted(&coordinator).await.is_empty());

    // A commit that stands keeps it, and the target session's activity moves.
    release.add_permits(1);
    let staging = agent_id.clone();
    coordinator
        .run_with_commit_waiting(
            room_request(&agent_id, "room-c", "three"),
            move |state, _| {
                assert!(state.stage_side_post(&staging, "chat:target", post(&staging, "p3")));
                // An unknown session stages nothing.
                assert!(!state.stage_side_post(&staging, "chat:missing", post(&staging, "p4")));
                Ok(())
            },
            |_| Ok(()),
        )
        .await
        .expect("the commit stands");
    entered.acquire().await.unwrap().forget();
    assert_eq!(posted(&coordinator).await, vec!["p3".to_string()]);
}
