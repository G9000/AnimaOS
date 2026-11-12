//! Owner chat control for agent talk, and dissolving an agent's helper team.
use super::*;

pub(crate) const SET_AGENT_TALK: &str = "set_agent_talk";
pub(crate) const DISSOLVE_HELPERS: &str = "dissolve_helpers";

pub(super) fn register(registry: &mut ToolRegistry) {
    let mut talk = object_parameters(vec![
        required_parameter(
            "agent",
            non_blank_string_parameter(
                "The agent to change: its name or ID from the roster, or \"self\" for you",
            ),
        ),
        required_parameter(
            "enabled",
            boolean_parameter("true lets it talk with other agents; false makes it work alone"),
        ),
    ]);
    talk.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(
        tool_descriptor(
            SET_AGENT_TALK,
            "Turn talking with other agents on or off for one agent when the owner asks, for example \"let Ada talk to the other agents\" or \"keep yourself to yourself\". Agents work alone by default; two agents can only message or ask each other when both have talking on. Saves the change at once; it applies from that agent's next run.",
            talk,
        ),
        execute_talk,
    );
    let mut dissolve = object_parameters(vec![]);
    dissolve.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(
        tool_descriptor(
            DISSOLVE_HELPERS,
            "Dissolve your temporary helper team once its job is done. Removes every one of your helpers that is not working right now; busy helpers stay. Their results already in this chat stay, but their own transcripts can no longer be opened. Your next spawn_helper starts a fresh team.",
            dissolve,
        ),
        execute_dissolve,
    );
}

fn done(result: Result<serde_json::Value, String>) -> TaskResult<Content> {
    match result {
        Ok(value) => TaskResult::success(
            Content {
                text: value.to_string(),
                ..Default::default()
            },
            0,
        ),
        Err(error) => TaskResult::error(error, 0),
    }
}

pub(super) fn execute_talk(
    context: ToolExecutionContext,
    agent: AgentState,
    user: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        done(
            async {
                if !context.standing_owner_turn
                    || context.has_upstream_tool_origin()
                    || crate::agent_runs::is_helper_config(&agent.config)
                    || user.agent_id != agent.id
                {
                    return Err(
                        "Changing who talks with agents requires a direct owner chat turn".into(),
                    );
                }
                if call
                    .args
                    .keys()
                    .any(|key| !matches!(key.as_str(), "agent" | "enabled"))
                {
                    return Err("Unknown set_agent_talk argument".into());
                }
                let target = match call.args.get("agent") {
                    Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim(),
                    _ => return Err("agent must be a name or ID".into()),
                };
                let target = if target.eq_ignore_ascii_case("self") {
                    agent.id.as_str()
                } else {
                    target
                };
                let Some(DataValue::Bool(enabled)) = call.args.get("enabled") else {
                    return Err("enabled must be true or false".into());
                };
                let coordinator = context
                    .team
                    .as_ref()
                    .ok_or("The agent runtime is unavailable")?;
                coordinator.set_agent_talk(target, *enabled).await
            }
            .await,
        )
    })
}

pub(super) fn execute_dissolve(
    context: ToolExecutionContext,
    agent: AgentState,
    _user: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        done(
            async {
                if !context.can_delegate {
                    return Err("Only an agent's own run can dissolve its helpers".into());
                }
                if !call.args.is_empty() {
                    return Err("dissolve_helpers takes no arguments".into());
                }
                let coordinator = context
                    .team
                    .as_ref()
                    .ok_or("The agent runtime is unavailable")?;
                coordinator.dissolve_helpers(&agent.id).await
            }
            .await,
        )
    })
}
