use super::*;

pub(super) fn register(registry: &mut ToolRegistry) {
    let mut file_parameters = object_parameters(vec![
        required_parameter("path", non_blank_string_parameter("Workspace-relative file path using /, for example reports/summary.md; one supported file up to 10 MiB")),
        optional_parameter("caption", string_parameter("Optional message accompanying the file, at most 2000 UTF-16 units")),
    ]);
    file_parameters.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(tool_descriptor("send_discord_file", "Upload a generated workspace file (such as a Markdown report) to this Discord thread. Create the file first with write_file. The host selects the destination. Wait for sent:true before confirming delivery; never automatically retry an uncertain upload. Supports validated text/source files, images, and common documents; voice/video is unavailable.", file_parameters), execute_file);
    let mut title = non_blank_string_parameter("Requested plain-text title for this Discord thread, at most 80 UTF-16 units (emoji use two); no control characters");
    if let DataValue::Object(schema) = &mut title {
        schema.insert("maxLength".into(), DataValue::Number(80.0));
    }
    let mut parameters = object_parameters(vec![required_parameter("title", title)]);
    parameters.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(tool_descriptor("rename_discord_thread", "Rename only the current Discord thread when a participant requests it. Can replace an existing/manual title. Takes no channel or connector ID. Call this tool and wait for renamed:true before confirming a title change. Errors and uncertain outcomes do not confirm a rename; do not retry uncertain effects automatically.", parameters), execute);
}

fn execute_file(
    context: ToolExecutionContext,
    agent: AgentState,
    user: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result: Result<String, String> = async {
            if call.args.keys().any(|key| !matches!(key.as_str(), "path" | "caption")) {
                return Err("send_discord_file accepts only path and caption; the host selects the current thread".into());
            }
            let path = match call.args.get("path") {Some(DataValue::String(value)) if !value.trim().is_empty() => value, _ => return Err("path must be a non-empty workspace-relative string".into())};
            let caption = match call.args.get("caption") {Some(DataValue::String(value)) => value.as_str(), None => "", _ => return Err("caption must be a string".into())};
            if context.has_upstream_tool_origin() || crate::agent_runs::is_helper_config(&agent.config) {
                return Err("Discord file sending requires a direct current-thread run".into());
            }
            let run = context.run_link.as_ref().filter(|run| run.agent_id == agent.id && user.agent_id == agent.id && run.session_id == user.room_id).ok_or("A live current-thread run is required")?;
            let scope = context.discord_thread.as_ref().ok_or("This run has no native Discord thread to upload to")?;
            let receipt = scope.send_file(run, path, caption, &call.id, context.cancel.clone()).await?;
            serde_json::to_string(&receipt).map_err(|e| e.to_string())
        }.await;
        match result {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    ..Default::default()
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

fn execute(
    context: ToolExecutionContext,
    agent: AgentState,
    user: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result: Result<String, String> = async {
            if call.args.keys().any(|key| key != "title") {
                return Err(
                    "rename_discord_thread accepts only title; the host selects the current thread"
                        .into(),
                );
            }
            let title = match call.args.get("title") {
                Some(DataValue::String(value)) => value,
                _ => return Err("title must be a string".into()),
            };
            if context.has_upstream_tool_origin()
                || crate::agent_runs::is_helper_config(&agent.config)
            {
                return Err("Discord renaming requires a direct current-thread run".into());
            }
            let run = context
                .run_link
                .as_ref()
                .filter(|run| {
                    run.agent_id == agent.id
                        && user.agent_id == agent.id
                        && run.session_id == user.room_id
                })
                .ok_or("A live current-thread run is required")?;
            let scope = context
                .discord_thread
                .as_ref()
                .ok_or("This run has no native Discord thread to rename")?;
            let receipt = scope.rename(run, title, context.cancel.clone()).await?;
            serde_json::to_string(&receipt).map_err(|e| e.to_string())
        }
        .await;
        match result {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    ..Default::default()
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}
