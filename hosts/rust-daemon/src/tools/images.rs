use super::*;

pub(super) fn register(registry: &mut ToolRegistry) {
    for (name,description) in [
        ("generate_image","Create one image from a prompt. Requires an explicit owner review before sending the prompt to OpenAI and incurring API charges. Saves a new local version beside this chat. Uses the saved OpenAI API key, not a ChatGPT subscription. Never claim visual inspection from the saved result."),
        ("edit_image","Edit exactly one PNG, JPEG or WebP image attached to the current owner message. Ask the owner to attach/select an image if none or several are attached. Requires exact owner review of the prompt, input image and OpenAI API charges. Saves a new version and preserves the original. No paths, source URLs, consent flags or model-supplied attachment IDs are accepted.")
    ] {
        let mut schema=object_parameters(vec![required_parameter("prompt",non_blank_string_parameter("Complete image creation or edit instruction, at most 2048 UTF-8 bytes"))]);
        schema.insert("additionalProperties".into(),DataValue::Bool(false));
        registry.register(tool_descriptor(name,description,schema),execute);
    }
}

fn execute(
    context: ToolExecutionContext,
    agent: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if let Err(result) = context.live_checks(&agent, &call).await {
            return result;
        }
        let (Some(coordinator), Some(approval), Some(cancel), Some(journal)) = (
            context.team.clone(),
            context.image_approval.clone(),
            context.cancel.clone(),
            context.action_journal.clone(),
        ) else {
            return TaskResult::error(
                "Image generation requires an exact approved direct owner run",
                0,
            );
        };
        if approval.witness.call != call {
            return TaskResult::error("Image request changed after owner review", 0);
        }
        let provider = match coordinator.image_provider(&approval).await {
            Ok(provider) => provider,
            Err(error) => return TaskResult::error(error, 0),
        };
        match coordinator.execute_image(approval,cancel,provider).await {
            Ok(value)=>TaskResult::success(Content {text:format!("Saved one new image version. This confirms storage, not visual inspection.\n{}",value),..Content::default()},0),
            Err(error)=>{
                if error.contains("outcome is unknown") {journal.stop_for_unproven_effects("Image request may have been charged; further effects were stopped");}
                TaskResult::error(error,0)
            }
        }
    })
}
