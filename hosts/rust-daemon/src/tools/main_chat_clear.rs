use super::*;

pub(super) fn execute(
    context: ToolExecutionContext,
    agent: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result: Result<(), String> = async {
            if !call.args.is_empty() {
                return Err("clear_main_chat takes no arguments".into());
            }
            if !context.standing_owner_turn
                || context.has_upstream_tool_origin()
                || crate::agent_runs::is_helper_config(&agent.config)
            {
                return Err("Clearing requires a direct local-owner main-chat request".into());
            }
            let run = context
                .run_link
                .as_ref()
                .filter(|r| r.agent_id == agent.id)
                .ok_or("A live owner run is required")?;
            context
                .team
                .as_ref()
                .ok_or("The chat runtime is unavailable")?
                .request_main_chat_clear(run)
                .await
        }
        .await;
        match result {
            Ok(()) => TaskResult::success(Content { text: "The clear request is saved. Finish this reply now; the host will clear this conversation after the turn settles, keeping the companion and saved memories.".into(), ..Default::default() }, 0),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}
