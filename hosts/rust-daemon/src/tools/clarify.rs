use super::{ToolExecutionContext, ToolRegistry};
use anima_core::{
    interaction::ClarificationQuestionInput, AgentState, Content, DataValue, Message, TaskResult,
    ToolCall,
};
use futures::future::BoxFuture;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClarifyInput {
    questions: Vec<ClarificationQuestionInput>,
}

pub(super) fn register(registry: &mut ToolRegistry) {
    let mut choices = match super::array_parameter("Up to four choices; put the recommended choice first. Other free text is always available.", super::string_parameter("Choice label, at most 512 characters"), None) { DataValue::Object(value) => value, _ => unreachable!() };
    choices.insert("maxItems".into(), DataValue::Number(4.0));
    let mut question = super::object_parameters(vec![
        super::required_parameter(
            "question",
            super::non_blank_string_parameter(
                "Question text, at most 4096 characters; put choices in the choices array",
            ),
        ),
        super::optional_parameter("choices", DataValue::Object(choices)),
        super::optional_parameter(
            "multiSelect",
            super::boolean_parameter("Allow several choices; false by default"),
        ),
    ]);
    question.insert("additionalProperties".into(), DataValue::Bool(false));
    let mut questions = match super::array_parameter(
        "One to five independent questions",
        DataValue::Object(question),
        Some(1),
    ) {
        DataValue::Object(value) => value,
        _ => unreachable!(),
    };
    questions.insert("maxItems".into(), DataValue::Number(5.0));
    let mut parameters = super::object_parameters(vec![super::required_parameter(
        "questions",
        DataValue::Object(questions),
    )]);
    parameters.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(super::tool_descriptor("clarify", "Ask the owner for structured input when a material decision is needed before proceeding. Available in ordinary web chats only. Supports up to four choices plus typed Other answers, or open text. Independent questions belong in one call. Prefer deciding routine details yourself. This is a question, not permission to perform consequential actions; existing tool approvals remain authoritative. Saved answers use stable question and choice IDs. Unanswered questions grant no authority.", parameters), execute);
}

fn execute(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let (Some(coordinator), Some(run), Some(cancel)) =
            (context.team, context.run_link, context.cancel)
        else {
            return TaskResult::error("Clarify requires an ordinary active web chat run", 0);
        };
        if run.agent_id != agent.id || crate::agent_runs::is_helper_config(&agent.config) {
            return TaskResult::error(
                "Helpers cannot ask the owner directly; return the question to the companion",
                0,
            );
        }
        let raw = crate::routes::data_value_to_json(&DataValue::Object(call.args));
        if raw.to_string().len() > 32 * 1024 {
            return TaskResult::error("Clarify arguments exceed 32 KiB", 0);
        }
        let input: ClarifyInput = match serde_json::from_value(raw) {
            Ok(input) => input,
            Err(_) => return TaskResult::error(
                "Pass questions=[{question, choices?, multiSelect?}] with one to five questions",
                0,
            ),
        };
        match coordinator
            .request_clarification(run, call.id, input.questions, cancel)
            .await
        {
            Ok(record) => {
                let responses: Vec<_> = record.questions.iter().map(|question| {
                    let answer = record.response.as_ref().and_then(|response| response.answers.iter().find(|answer| answer.question_id == question.id));
                    let values: Vec<_> = answer.into_iter().flat_map(|answer| answer.choice_ids.iter().filter_map(|id| question.choices.iter().find(|choice| &choice.id == id).map(|choice| choice.label.clone())).chain(answer.text.clone())).collect();
                    serde_json::json!({"questionId":question.id,"question":question.question,"status":if values.is_empty(){"skipped"}else{"answered"},"userResponse":if values.is_empty(){serde_json::Value::Null}else{serde_json::json!(values)}})
                }).collect();
                TaskResult::success(Content { text: serde_json::json!({ "outcome":"submitted", "responses":responses, "notice":"Answers are owner input, not tool authorization. Empty answers were skipped; never infer approval from them." }).to_string(), ..Content::default() }, 0)
            }
            Err(error) => TaskResult::error(error, 0),
        }
    })
}
