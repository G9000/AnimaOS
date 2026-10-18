use super::ToolExecutionContext;
use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;

pub(super) fn send_message(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let (Some(coordinator), Some(route)) = (context.team, context.peer_route) else {
            return TaskResult::error("Peer communication requires a workspace agent run", 0);
        };
        let arg = |key: &str| match call.args.get(key) {
            Some(DataValue::String(value)) if !value.is_empty() => Some(value.as_str()),
            _ => None,
        };
        let Some(message) = arg("message") else {
            return TaskResult::error("message is required", 0);
        };
        let target = match coordinator
            .resolve_peer(arg("to_agent_id"), arg("to_agent_name"))
            .await
        {
            Ok(target) => target,
            Err(error) => return TaskResult::error(error, 0),
        };
        match coordinator
            .send_peer(agent.id, target.id.clone(), message.into(), route, context.run_link.clone())
            .await
        {
            Ok(result) => TaskResult::success(Content { text: serde_json::json!({"fromAgentId": target.id, "fromAgentName": target.name, "status": result.result.status, "result": result.result.data, "error": result.result.error}).to_string(), ..Content::default() }, 0),
            Err(error) => TaskResult::error(format!("Peer request failed: {}", error.message()), 0),
        }
    })
}

pub(super) fn broadcast_message(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let (Some(coordinator), Some(route)) = (context.team, context.peer_route) else {
            return TaskResult::error("Peer communication requires a workspace agent run", 0);
        };
        let Some(DataValue::String(message)) = call.args.get("message") else {
            return TaskResult::error("message is required", 0);
        };
        if message.is_empty() {
            return TaskResult::error("message is required", 0);
        }
        let mut deliveries = vec![];
        for target in coordinator
            .peer_ids()
            .await
            .into_iter()
            .filter(|id| !route.participants().contains(id))
        {
            let delivery = match coordinator
                .send_peer(
                    agent.id.clone(),
                    target.clone(),
                    message.clone(),
                    route.clone(),
                    context.run_link.clone(),
                )
                .await
            {
                Ok(result) => {
                    serde_json::json!({"agentId": target, "status": result.result.status, "result": result.result.data, "error": result.result.error})
                }
                Err(error) => {
                    serde_json::json!({"agentId": target, "status": "error", "error": error.message()})
                }
            };
            deliveries.push(delivery);
        }
        TaskResult::success(
            Content {
                text: serde_json::json!({"deliveries": deliveries}).to_string(),
                ..Content::default()
            },
            0,
        )
    })
}

pub(super) fn list_workspace_agents(
    context: ToolExecutionContext,
    _agent: AgentState,
    _message: Message,
    _call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let Some(coordinator) = context.team else {
            return TaskResult::error(
                "Workspace roster is unavailable in this execution context",
                0,
            );
        };
        TaskResult::success(
            Content {
                text: coordinator.team_roster().await,
                attachments: None,
                metadata: None,
            },
            0,
        )
    })
}

pub(super) fn delegate_to_agent(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if !context.can_delegate {
            return TaskResult::error(
                "Only the workspace manager can delegate; recursive delegation is not allowed",
                0,
            );
        }
        let Some(coordinator) = context.team else {
            return TaskResult::error("Delegation is unavailable in this execution context", 0);
        };
        let arg = |key: &str| match call.args.get(key) {
            Some(DataValue::String(value)) if !value.trim().is_empty() => {
                Some(value.trim().to_owned())
            }
            _ => None,
        };
        let (Some(target), Some(task)) = (arg("agent_id"), arg("task")) else {
            return TaskResult::error("agent_id and task are required", 0);
        };
        match coordinator
            .delegate(&agent, target, task, context.run_link.clone())
            .await
        {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(super) fn spawn_helper(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if !context.can_delegate {
            return TaskResult::error(
                "Only the companion can spawn helpers; recursive or peer spawning is not allowed",
                0,
            );
        }
        let Some(coordinator) = context.team else {
            return TaskResult::error(
                "Helper creation is unavailable in this execution context",
                0,
            );
        };
        let arg = |key: &str| match call.args.get(key) {
            Some(DataValue::String(value)) => Some(value.trim().to_owned()),
            _ => None,
        };
        let (Some(name), Some(task)) = (arg("name"), arg("task")) else {
            return TaskResult::error("name and task are required", 0);
        };
        let worktree = match worktree_request(&call) {
            Ok(worktree) => worktree,
            Err(error) => return TaskResult::error(error, 0),
        };
        if context
            .helper_starts
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |count| (count < 4).then_some(count + 1),
            )
            .is_err()
        {
            return TaskResult::error(
                "The four-helper start limit for this companion run has been reached",
                0,
            );
        }
        match coordinator
            .spawn_helper_in(agent.id, name, task, context.run_link.clone(), worktree)
            .await
        {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    ..Content::default()
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(super) fn async_helper_operation(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if !context.can_delegate {
            return TaskResult::error("Only the companion's ordinary run can manage helpers", 0);
        }
        let (Some(coordinator), Some(parent)) = (context.team, context.run_link) else {
            return TaskResult::error(
                "Helper operations require an authentic active companion run",
                0,
            );
        };
        let result = if call.name == "spawn_helper_async" {
            let arg = |key: &str| match call.args.get(key) {
                Some(DataValue::String(value)) => Some(value.clone()),
                _ => None,
            };
            let (Some(name), Some(task)) = (arg("name"), arg("task")) else {
                return TaskResult::error("name and task are required", 0);
            };
            let completion = match call.args.get("completion") {
                None => "chat".to_string(),
                Some(DataValue::String(value))
                    if matches!(value.as_str(), "chat" | "manual" | "origin") =>
                {
                    value.clone()
                }
                _ => {
                    return TaskResult::error(
                        "completion must be chat, manual or supported job origin",
                        0,
                    )
                }
            };
            let worktree = match worktree_request(&call) {
                Ok(worktree) => worktree,
                Err(error) => return TaskResult::error(error, 0),
            };
            coordinator
                .accept_async_helper(
                    agent.id,
                    parent,
                    call.id,
                    name,
                    task,
                    completion,
                    context.helper_starts,
                    worktree,
                )
                .await
        } else {
            coordinator
                .manage_async_helper(agent.id, parent, call)
                .await
        };
        match result {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    ..Content::default()
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

impl ToolExecutionContext {
    /// Points a worktree helper's tools at its own checkout.
    pub(crate) fn with_workspace_root(mut self, root: Option<std::path::PathBuf>) -> Self {
        self.workspace_root = root;
        self
    }
}

/// `worktree`/`repo` arguments: `Some(repo)` when a worktree is requested.
fn worktree_request(call: &ToolCall) -> Result<Option<String>, String> {
    let requested = match call.args.get("worktree") {
        None => false,
        Some(DataValue::Bool(value)) => *value,
        Some(_) => return Err("worktree must be a boolean".into()),
    };
    let repo = match call.args.get("repo") {
        None => None,
        Some(DataValue::String(value)) if value.len() <= 1024 => Some(value.trim().to_owned()),
        Some(_) => return Err("repo must be a workspace-relative path string".into()),
    };
    match (requested, repo) {
        (true, repo) => Ok(Some(
            repo.filter(|repo| !repo.is_empty())
                .unwrap_or_else(|| ".".into()),
        )),
        (false, Some(_)) => Err("repo is only used with worktree: true".into()),
        (false, None) => Ok(None),
    }
}
