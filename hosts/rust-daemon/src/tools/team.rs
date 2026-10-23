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

/// One assignment input feeds both native launch paths and the existing async
/// replay fingerprint. Validate before reserving starts or saving a child.
pub(super) fn helper_assignment(call: &ToolCall) -> Result<String, String> {
    let task = match call.args.get("task") {
        Some(DataValue::String(task)) if !task.trim().is_empty() && task.len() <= 32_768 => task,
        _ => return Err("task must be nonblank and at most 32768 bytes".into()),
    };
    let playbook = match call.args.get("playbook") {
        None => return Ok(task.clone()),
        Some(DataValue::String(playbook)) if !playbook.trim().is_empty() => playbook,
        _ => return Err("playbook must be a nonblank string".into()),
    };
    let assignment = format!("## Assigned task\n{task}\n\n## Assignment playbook\n{playbook}");
    if assignment.len() > 32_768 {
        return Err("task and playbook together must fit within 32768 bytes".into());
    }
    Ok(assignment)
}

/// Pure launch validation used to preflight every item before a batch mutates state.
pub(super) fn validate_async_helper_launch(call: &ToolCall) -> Result<(), String> {
    match call.args.get("name") {
        Some(DataValue::String(v)) if !v.trim().is_empty() && v.len() <= 80 => (),
        _ => return Err("name must be nonblank and at most 80 bytes".into()),
    }
    let task = helper_assignment(call)?;
    let options = crate::agent_runs::helper_options::HelperOptions::from_call(call)?;
    options.task(&task)?;
    worktree_request(call)?;
    match call.args.get("completion") {
        None => (),
        Some(DataValue::String(v)) if matches!(v.as_str(), "chat" | "manual" | "origin") => (),
        _ => return Err("completion must be chat, manual or origin".into()),
    }
    Ok(())
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
        let Some(name) = arg("name").filter(|name| !name.is_empty() && name.len() <= 80) else {
            return TaskResult::error("name is required", 0);
        };
        let task = match helper_assignment(&call) {
            Ok(task) => task,
            Err(error) => return TaskResult::error(error, 0),
        };
        let worktree = match worktree_request(&call) {
            Ok(worktree) => worktree,
            Err(error) => return TaskResult::error(error, 0),
        };
        let options = match crate::agent_runs::helper_options::HelperOptions::from_call(&call) {
            Ok(v) => v,
            Err(e) => return TaskResult::error(e, 0),
        };
        if let Err(e) = options.task(&task) {
            return TaskResult::error(e, 0);
        }
        let max_starts = match coordinator
            .validate_helper_options(&agent.id, &options)
            .await
        {
            Ok(v) => v,
            Err(e) => return TaskResult::error(e, 0),
        };
        if context
            .helper_starts
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |count| (count < max_starts).then_some(count + 1),
            )
            .is_err()
        {
            return TaskResult::error(
                "The configured helper start limit for this companion run has been reached",
                0,
            );
        }
        match coordinator
            .spawn_helper_with_options(
                agent.id,
                name,
                task,
                context.run_link.clone(),
                worktree,
                options,
            )
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
    message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    async_helper_operation_for(context, agent, message, call, false)
}

pub(super) fn async_helper_batch_item(
    context: ToolExecutionContext,
    agent: AgentState,
    message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    if !agent.config.allows_tool("spawn_helper_async") || !agent.config.allows_tool("spawn_helpers")
    {
        return Box::pin(async {
            TaskResult::error(
                "Batch launch requires both captured background helper grants",
                0,
            )
        });
    }
    async_helper_operation_for(context, agent, message, call, true)
}

fn async_helper_operation_for(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
    batch: bool,
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
            if let Err(e) = validate_async_helper_launch(&call) {
                return TaskResult::error(e, 0);
            }
            let options = match crate::agent_runs::helper_options::HelperOptions::from_call(&call) {
                Ok(v) => v,
                Err(e) => return TaskResult::error(e, 0),
            };
            let arg = |key: &str| match call.args.get(key) {
                Some(DataValue::String(value)) => Some(value.clone()),
                _ => None,
            };
            let Some(name) = arg("name") else {
                return TaskResult::error("name is required", 0);
            };
            let task = match helper_assignment(&call) {
                Ok(task) => task,
                Err(error) => return TaskResult::error(error, 0),
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
                .accept_async_helper_with_options(
                    agent.id,
                    parent,
                    call.id,
                    name,
                    task,
                    completion,
                    context.helper_starts,
                    worktree,
                    options,
                    batch,
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
