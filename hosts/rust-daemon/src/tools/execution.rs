//! Ordinary native tools: grants, Exec approvals, and durable run receipts apply upstream.
use super::*;
use crate::execution_backends::{ExecutionRequest, ExecutionStatus};

pub(super) fn register(registry: &mut ToolRegistry) {
    registry.register(tool_descriptor("execution_targets","List immutable operator-configured Local, Docker and SSH execution targets. Target IDs select fixed transport profiles; an unavailable target never falls back to local execution.",object_parameters(vec![])),targets);
    let mut schema = object_parameters(vec![
        required_parameter(
            "targetId",
            non_blank_string_parameter("Exact configured execution target ID"),
        ),
        required_parameter(
            "command",
            non_blank_string_parameter("Command data, at most 16384 bytes"),
        ),
        optional_parameter(
            "cwd",
            string_parameter(
                "Workspace-relative cwd, default .; Docker uses its isolated /workspace",
            ),
        ),
        optional_parameter(
            "timeoutMs",
            bounded_integer_parameter(
                "Command deadline in milliseconds, default 30000, capped by the configured target",
                1,
                300_000,
            ),
        ),
    ]);
    schema.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(tool_descriptor("execution_run","Execute command data on one explicit operator-configured target through ordinary execution approval and receipts. Only targetId, command, cwd and timeoutMs are accepted. Docker uses an installed digest-pinned image, no host mounts or network, unprivileged read-only root and bounded resources. SSH uses a fixed pinned identity and Rust worker. outcomeUnknown retains target capacity until authoritative reconciliation; effects may have occurred.",schema),run);
    let mut schema = object_parameters(vec![required_parameter(
        "executionId",
        non_blank_string_parameter("Exact execution ID returned by execution_run"),
    )]);
    schema.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(tool_descriptor("execution_status","Read the current native receipt for an exact execution ID, including authoritative Docker/SSH cleanup reconciliation. Unknown outcomes are not success evidence.",schema),status);
}
fn targets(
    context: ToolExecutionContext,
    _: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if !call.args.is_empty() {
            return TaskResult::error("execution_targets accepts no arguments", 0);
        }
        content(serde_json::to_string(&context.execution.targets()).unwrap())
    })
}
fn status(
    context: ToolExecutionContext,
    agent: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if call.args.len() != 1 {
            return TaskResult::error("execution_status requires only executionId", 0);
        }
        let Some(DataValue::String(id)) = call.args.get("executionId") else {
            return TaskResult::error("execution_status requires executionId", 0);
        };
        match context.execution.receipt_for_agent(id, &agent.id) {
            Some(receipt) => content(serde_json::to_string(&receipt).unwrap()),
            None => TaskResult::error("Execution receipt is unavailable", 0),
        }
    })
}
fn run(
    context: ToolExecutionContext,
    agent: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if let Err(result) = context.live_checks(&agent, &call).await {
            return result;
        }
        let Some(journal) = context.action_journal.clone() else {
            return TaskResult::error(
                "Execution requires an ordinary owned native action journal",
                0,
            );
        };
        let call_for_authority = call.clone();
        let approved = context.action_approved;
        let scope = crate::execution_backends::ExecutionScope {
            agent_id: agent.id.clone(),
            authority: Arc::new(move || {
                let journal = journal.clone();
                let call = call_for_authority.clone();
                Box::pin(async move { journal.execution_authority(&call, approved).await })
            }),
        };
        let request:ExecutionRequest=match parse_request(call.args){Ok(request)=>request,Err(_)=>return TaskResult::error("execution_run accepts only targetId, command, cwd and timeoutMs with their declared types",0)};
        match context
            .execution
            .execute_scoped(request, context.cancel.clone(), scope)
            .await
        {
            Ok(receipt) => {
                let text = serde_json::to_string(&receipt).unwrap();
                if receipt.status == ExecutionStatus::OutcomeUnknown || !receipt.cleanup_confirmed {
                    if let Some(journal) = context.action_journal.as_ref() {
                        journal.stop_for_unproven_effects("Execution outcome or physical cleanup is unknown; further effects were stopped");
                    }
                }
                if receipt.status == ExecutionStatus::Succeeded {
                    content(text)
                } else {
                    TaskResult::error(text, 0)
                }
            }
            Err(error) => TaskResult::error(error, 0),
        }
    })
}
fn parse_request(args: BTreeMap<String, DataValue>) -> Result<ExecutionRequest, String> {
    let timeout = match args.get("timeoutMs") {
        Some(DataValue::Number(value))
            if value.is_finite()
                && *value >= 1.0
                && *value <= 300_000.0
                && value.fract() == 0.0 =>
        {
            Some(*value as u64)
        }
        Some(_) => return Err("timeoutMs must be a positive bounded integer".into()),
        None => None,
    };
    let mut value = crate::routes::data_value_to_json(&DataValue::Object(args));
    if let Some(timeout) = timeout {
        value["timeoutMs"] = serde_json::json!(timeout);
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}
fn content(text: String) -> TaskResult<Content> {
    TaskResult::success(
        Content {
            text,
            attachments: None,
            metadata: None,
        },
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn execution_native_arguments_accept_integral_runtime_numbers_and_refuse_transport_overrides() {
        let mut args = BTreeMap::from([
            ("targetId".into(), DataValue::String("configured".into())),
            ("command".into(), DataValue::String("echo hi".into())),
            ("timeoutMs".into(), DataValue::Number(250.0)),
        ]);
        assert_eq!(parse_request(args.clone()).unwrap().timeout_ms, 250);
        args.insert("timeoutMs".into(), DataValue::Number(1.5));
        assert!(parse_request(args.clone()).is_err());
        args.insert("timeoutMs".into(), DataValue::Number(250.0));
        args.insert("executable".into(), DataValue::String("evil".into()));
        assert!(parse_request(args).is_err());
    }
}
