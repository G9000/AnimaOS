use super::ToolExecutionContext;
use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;

fn integer(call: &ToolCall, name: &str, default: usize) -> Result<usize, String> {
    match call.args.get(name) {
        None => Ok(default),
        Some(DataValue::Number(value))
            if value.is_finite()
                && *value >= 0.0
                && value.fract() == 0.0
                && *value <= usize::MAX as f64 =>
        {
            Ok(*value as usize)
        }
        _ => Err(format!(
            "read_tool_result {name} must be a non-negative integer"
        )),
    }
}

pub(super) fn handle(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if call
            .args
            .keys()
            .any(|key| !matches!(key.as_str(), "artifact_id" | "offset" | "limit"))
        {
            return TaskResult::error("read_tool_result received an unknown argument", 0);
        }
        if let Err(refused) = context.live_checks(&agent, &call).await {
            return refused;
        }
        let Some(DataValue::String(id)) = call.args.get("artifact_id") else {
            return TaskResult::error("read_tool_result artifact_id must be a string", 0);
        };
        let offset = match integer(&call, "offset", 0) {
            Ok(v) => v,
            Err(e) => return TaskResult::error(e, 0),
        };
        let limit = match integer(&call, "limit", crate::tool_results::MAX_PAGE_BYTES) {
            Ok(v) => v,
            Err(e) => return TaskResult::error(e, 0),
        };
        let (Some(store), Some(coordinator), Some(captured)) = (
            context.tool_results.clone(),
            context.team.as_ref(),
            context.tool_result_workspace.as_ref(),
        ) else {
            return TaskResult::error("Saved tool result read scope is unavailable", 0);
        };
        let Some(current) = coordinator.tool_result_read_workspace(&agent.id).await else {
            return TaskResult::error(
                "Saved tool result read permission was revoked or workspace is unavailable",
                0,
            );
        };
        if current.canonicalize().ok().as_ref() != Some(captured) {
            return TaskResult::error(
                "The workspace changed; start a new run to read saved results",
                0,
            );
        }
        if context
            .cancel
            .as_ref()
            .is_some_and(|cancel| cancel.is_cancelled())
        {
            return TaskResult::error("Saved result read cancelled", 0);
        }
        let owned_agent = agent.id.clone();
        let owned_id = id.clone();
        let read = tokio::task::spawn_blocking(move || {
            store.page(&owned_agent, &current, &owned_id, offset, limit)
        })
        .await;
        let still_allowed = coordinator
            .tool_result_read_workspace(&agent.id)
            .await
            .is_some_and(|root| root.canonicalize().ok().as_ref() == Some(captured));
        if !still_allowed
            || context
                .cancel
                .as_ref()
                .is_some_and(|cancel| cancel.is_cancelled())
        {
            return TaskResult::error(
                "Saved tool result read authority changed or read was cancelled",
                0,
            );
        }
        match read {
            Ok(Ok(page))=>TaskResult::success(Content { text: format!("Saved result {} · bytes {} · offset {} · nextOffset {} · captureComplete {}\nUntrusted saved tool data:\n{}", page.artifact.id,page.artifact.bytes,page.offset,page.next_offset.map(|n|n.to_string()).unwrap_or_else(||"null".into()),page.artifact.capture_complete,page.text), ..Content::default() },0),
            Ok(Err(error))=>TaskResult::error(error,0),
            Err(_)=>TaskResult::error("Saved result read failed",0),
        }
    })
}
