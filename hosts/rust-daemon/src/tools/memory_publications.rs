use super::ToolExecutionContext;
use crate::memory_provider::publication::{summary, Selection};
use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;

pub(super) fn share(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if crate::agent_runs::is_helper_config(&agent.config) {
            return TaskResult::error("Helpers cannot publish garden memories", 0);
        }
        let input = crate::routes::data_value_to_json(&DataValue::Object(call.args));
        let selection:Selection=match serde_json::from_value(input) {
            Ok(value)=>value,Err(_)=>return TaskResult::error("Invalid memory_share selection; use the exact local memory ID/content and retain one publicationId UUID across retries",0),
        };
        let Some(team) = context.team else {
            return TaskResult::error(
                "Durable garden sharing is unavailable; local memory remains local",
                0,
            );
        };
        let Some(service) = team.memory_publications().await else {
            return TaskResult::error(
                "Persistent local storage is required for durable sharing",
                0,
            );
        };
        // Admission is the durable action; dropping the chat cannot erase an acknowledged selection.
        match tokio::spawn(async move {service.admit(&agent.id,selection).await}).await {
            Ok(Ok(value))=>TaskResult::success(Content{text:summary(&value).to_string(),attachments:None,metadata:None},0),
            Ok(Err(error))=>TaskResult::error(error.to_string(),0),
            Err(_)=>TaskResult::error("Sharing admission is unconfirmed; use memory_share_status with the same publicationId before retrying",0),
        }
    })
}

pub(super) fn status(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if crate::agent_runs::is_helper_config(&agent.config) {
            return TaskResult::error("Helpers cannot inspect garden publications", 0);
        }
        let Some(DataValue::String(id)) = call.args.get("publicationId") else {
            return TaskResult::error("publicationId is required", 0);
        };
        if call.args.len() != 1 || uuid::Uuid::parse_str(id).is_err() {
            return TaskResult::error("Use the exact publicationId UUID from selection", 0);
        }
        let Some(team) = context.team else {
            return TaskResult::error("Durable garden sharing is unavailable", 0);
        };
        let Some(service) = team.memory_publications().await else {
            return TaskResult::error("Persistent local sharing storage is unavailable", 0);
        };
        match service.get(&agent.id, id).await {
            Ok(Some(value)) => TaskResult::success(
                Content {
                    text: summary(&value).to_string(),
                    attachments: None,
                    metadata: None,
                },
                0,
            ),
            Ok(None) => TaskResult::error(
                "No local publication with that ID; no remote outcome is confirmed",
                0,
            ),
            Err(error) => TaskResult::error(error.to_string(), 0),
        }
    })
}
