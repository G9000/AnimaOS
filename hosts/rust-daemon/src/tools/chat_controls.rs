use super::*;

const NAMES: &[&str] = &["rename_chat", "compact_chat", "export_chat", "change_look"];

/// What each `change_look` choice means, for the model.
fn look_description(name: &str) -> &'static str {
    match name {
        "view" => "simple: the plain everyday console (work hidden, basic input); advanced: show each tool step and the Do/Plan and model controls",
        "theme" => "Colour theme",
        "accent" => "Highlight colour used for the orb, buttons and anything that needs the owner: pink, lavender, amber, sage, coral or paper, or any other colour as #rrggbb (teal is #14b8a6); custom brings back the owner's own colour",
        "textSize" => "Chat and settings text size",
        "density" => "Space between messages and rows",
        "chatBackground" => "What sits behind the chat: the full orb, a quiet orb at the edge, or nothing",
        "reduceMotion" => "Calmer orb and fewer animations",
        "advancedInput" => "Do/Plan and the model picker on the chat input",
        _ => "Each tool step, timing and result while the agent works; off shows one plain status line",
    }
}

pub(super) fn register(registry: &mut ToolRegistry) {
    let mut title = non_blank_string_parameter(
        "New title for this conversation, 1 to 120 plain-text characters",
    );
    if let DataValue::Object(schema) = &mut title {
        schema.insert("maxLength".into(), DataValue::Number(120.0));
    }
    let mut focus = string_parameter("Optional summary focus, at most 500 characters; required safety and task continuity are preserved");
    if let DataValue::Object(schema) = &mut focus {
        schema.insert("maxLength".into(), DataValue::Number(500.0));
    }
    for (name, description, parameters) in [
        (NAMES[0], "Rename this owner conversation when requested. Saves the validated title before confirming. Cannot target another conversation.", vec![required_parameter("title", title)]),
        (NAMES[1], "Compact earlier history of this owner conversation when requested. Keeps the latest complete exchanges and full exportable transcript; defaults to one kept exchange. Focus is untrusted guidance. Cannot target another conversation.", vec![optional_parameter("keepLatestExchanges", bounded_integer_parameter("Complete latest exchanges to keep, default 1", 1, 100)), optional_parameter("focus", focus)]),
        (NAMES[2], "Prepare an authenticated local Markdown download of this conversation's full transcript. Returns a relative daemon download URL and filename. It does not upload, send, or confirm a download.", vec![]),
    ] {
        let mut schema = object_parameters(parameters);
        schema.insert("additionalProperties".into(), DataValue::Bool(false));
        registry.register(tool_descriptor(name, description, schema), execute);
    }
    let parameters = crate::owner_look::LOOK_CHOICES
        .iter()
        .map(|(name, values)| {
            optional_parameter(name, string_enum_parameter(look_description(name), values))
        })
        .chain(std::iter::once(optional_parameter(
            "accent",
            string_parameter(look_description("accent")),
        )))
        .chain(
            crate::owner_look::LOOK_SWITCHES
                .iter()
                .map(|name| optional_parameter(name, boolean_parameter(look_description(name)))),
        )
        .collect();
    let mut schema = object_parameters(parameters);
    schema.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(
        tool_descriptor(
            NAMES[3],
            "Change how the owner's anima console looks when they ask, for example \"show me the advanced stuff\", \"make it simple again\", \"switch to light mode\" or \"make the accent lavender\". When the owner asks to change the colour, theme, background, text or how things look without naming a specific file, page or document, they mean this app: use this tool instead of asking which. Applies at once on every open console and is undoable from the chat. Pass only what should change; with no arguments it reports the current look without changing anything. Only the listed choices exist, except that the accent can be any colour. Cannot change agent behaviour or settings.",
            schema,
        ),
        execute,
    );
}

fn options(call: &ToolCall) -> Result<crate::sessions::compaction::CompactionOptions, String> {
    if call
        .args
        .keys()
        .any(|key| !matches!(key.as_str(), "keepLatestExchanges" | "focus"))
    {
        return Err("Unknown compact_chat argument".into());
    }
    let keep = match call.args.get("keepLatestExchanges") {
        None => None,
        Some(DataValue::Number(value))
            if value.is_finite() && value.fract() == 0.0 && (1.0..=100.0).contains(value) =>
        {
            Some(*value as usize)
        }
        _ => return Err("keepLatestExchanges must be an integer from 1 to 100".into()),
    };
    let focus = match call.args.get("focus") {
        None => None,
        Some(DataValue::String(value)) => Some(value.clone()),
        _ => return Err("focus must be a string".into()),
    };
    crate::sessions::compaction::CompactionOptions::new(keep, focus).map_err(str::to_string)
}

pub(super) fn execute(
    context: ToolExecutionContext,
    agent: AgentState,
    user: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result: Result<String, String> = async {
            if !context.standing_owner_turn || context.has_upstream_tool_origin()
                || crate::agent_runs::is_helper_config(&agent.config) || user.agent_id != agent.id {
                return Err("Chat controls require a direct owner web/API chat turn".into());
            }
            let run = context.run_link.as_ref().filter(|run| run.agent_id == agent.id).ok_or("A live owner run is required")?;
            let coordinator = context.team.as_ref().ok_or("The chat runtime is unavailable")?;
            match call.name.as_str() {
                "rename_chat" => {
                    if call.args.keys().any(|key| key != "title") { return Err("Unknown rename_chat argument".into()); }
                    let title = match call.args.get("title") { Some(DataValue::String(value)) => value, _ => return Err("title must be a string".into()) };
                    let title = coordinator.rename_owner_chat(run, title).await?;
                    Ok(serde_json::json!({"renamed":true,"title":title}).to_string())
                }
                "compact_chat" => {
                    let options = options(&call)?;
                    coordinator.compact_owner_chat(run, &options, context.cancel.clone()).await?;
                    Ok("Earlier conversation history was compacted and saved. The full transcript remains available for export.".into())
                }
                "export_chat" => {
                    if !call.args.is_empty() { return Err("export_chat takes no arguments".into()); }
                    serde_json::to_string(&coordinator.export_owner_chat(run).await?).map_err(|error| error.to_string())
                }
                "change_look" => Ok(coordinator.change_owner_look(run, call.args.clone()).await?.to_string()),
                _ => Err("Unknown chat control".into()),
            }
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
