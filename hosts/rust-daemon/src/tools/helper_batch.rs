//! Ordered independent launches through the ordinary durable helper API.
//! Each index has a stable operation identity. A batch is never atomic: exact
//! items replay, changed items conflict, and previously refused items may retry.
use super::ToolExecutionContext;
use anima_core::{AgentState, Content, DataValue, Message, TaskResult, TaskStatus, ToolCall};
use futures::future::BoxFuture;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const MAX_BATCH_INPUT_BYTES: usize = 64 * 1024;
const MAX_BATCH_RESPONSE_BYTES: usize = 32 * 1024;

pub(super) fn register(registry: &mut super::ToolRegistry) {
    // Register after the native single-launch tool so its current exact schema
    // (including route/limits/output contracts) is also the batch item schema.
    let launch_schema = registry
        .descriptor("spawn_helper_async")
        .expect("register helper collection after native async launch")
        .parameters_schema;
    let tasks = DataValue::Object(BTreeMap::from([
        ("type".into(), DataValue::String("array".into())),
        ("minItems".into(), DataValue::Number(1.0)),
        ("maxItems".into(), DataValue::Number(8.0)),
        ("items".into(), DataValue::Object(launch_schema)),
        ("description".into(), DataValue::String("One to eight native async helper launch requests; completion defaults to manual for each item".into())),
    ]));
    let mut batch_schema =
        super::object_parameters(vec![super::required_parameter("tasks", tasks)]);
    batch_schema.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(
        super::tool_descriptor(
            "spawn_helpers",
            "Accept one to eight independent asynchronous helper assignments in request order. Requires both this tool and spawn_helper_async grants. Every input shape is validated before launching; owner-policy admission can refuse individual items. Combined JSON input must fit 64 KiB. Each omitted completion defaults to manual. Returns one indexed receipt or refusal per item, including partial acceptance; the batch is not atomic. Exact retries replay accepted items, changed items at the same index are refused, and previously refused items may be tried again. Existing start, concurrency and execution limits remain shared. Use wait_helper_runs to collect saved results.",
            batch_schema,
        ),
        spawn_helpers,
    );
    let run_ids = DataValue::Object(BTreeMap::from([
        ("type".into(), DataValue::String("array".into())),
        ("minItems".into(), DataValue::Number(1.0)),
        ("maxItems".into(), DataValue::Number(8.0)),
        ("uniqueItems".into(), DataValue::Bool(true)),
        (
            "items".into(),
            super::non_blank_string_parameter(
                "Exact immutable helper run handle, at most 200 bytes",
            ),
        ),
    ]));
    let mut wait_schema = super::object_parameters(vec![
        super::required_parameter("run_ids", run_ids),
        super::optional_parameter(
            "mode",
            super::string_enum_parameter("Wait condition, default all", &["any", "all"]),
        ),
        super::optional_parameter(
            "timeout_ms",
            super::bounded_integer_parameter(
                "Maximum completion wait in milliseconds, default 10000; zero returns one snapshot",
                0,
                30_000,
            ),
        ),
    ]);
    wait_schema.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(
        super::tool_descriptor(
            "wait_helper_runs",
            "Collect one to eight unique owned immutable helper run handles, waiting for any or all saved terminal outcomes. Defaults to all with a 10000 ms completion wait; timeout_ms is bounded to 0..30000 and zero reads a snapshot. Results come only from committed helper replies or terminal failures, never streamed claims. Live events wake collection; local history lookup is separately bounded. Caller cancellation stops the wait. This operation starts no work and does not cancel pending children on timeout. Returns every handle, readiness, failures and explicit output truncation within 32 KiB.",
            wait_schema,
        ),
        wait_helper_runs,
    );
}

fn batch_calls(context: &ToolExecutionContext, call: &ToolCall) -> Result<Vec<ToolCall>, String> {
    if call.id.trim().is_empty() || call.id.len() > 4096 {
        return Err("A host tool-call identity is required".into());
    }
    if call.args.len() != 1 || !call.args.contains_key("tasks") {
        return Err("tasks is the only supported batch argument".into());
    }
    let input = crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()));
    if input.to_string().len() > MAX_BATCH_INPUT_BYTES {
        return Err("Combined helper batch input must fit 65536 bytes".into());
    }
    let Some(DataValue::Array(tasks)) = call.args.get("tasks") else {
        return Err("tasks must contain between 1 and 8 launch requests".into());
    };
    if !(1..=8).contains(&tasks.len()) {
        return Err("tasks must contain between 1 and 8 launch requests".into());
    }
    let launch = context
        .rpc_descriptor("spawn_helper_async")
        .ok_or("The native async helper launch schema is unavailable")?;
    let Some(DataValue::Object(properties)) = launch.parameters_schema.get("properties") else {
        return Err("The native async helper launch schema is unavailable".into());
    };
    tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let DataValue::Object(args) = task else {
                return Err(format!(
                    "tasks[{index}] must be a native async helper launch object"
                ));
            };
            if args.keys().any(|key| !properties.contains_key(key)) {
                return Err(format!("tasks[{index}] has an unsupported launch argument"));
            }
            let mut args = args.clone();
            args.entry("completion".into())
                .or_insert_with(|| DataValue::String("manual".into()));
            let identity =
                serde_json::to_vec(&json!(["anima.helper-batch.item.v1", call.id, index,]))
                    .expect("batch identity tuple serializes");
            let child = ToolCall {
                id: format!("helper-batch:{}", hex::encode(Sha256::digest(identity))),
                name: "spawn_helper_async".into(),
                args,
            };
            // Pure single-launch validation is shared with native dispatch.
            // No starts, agents, rooms, worktrees or durable writes occur here.
            super::team::validate_async_helper_launch(&child)
                .map_err(|error| format!("tasks[{index}]: {error}"))?;
            Ok(child)
        })
        .collect()
}

pub(super) fn spawn_helpers(
    context: ToolExecutionContext,
    agent: AgentState,
    message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let calls = match batch_calls(&context, &call) {
            Ok(calls) => calls,
            Err(error) => return TaskResult::error(error, 0),
        };
        if !context.can_delegate {
            return TaskResult::error("Only an authorized coordinator run can launch helpers", 0);
        }
        let mut entries = Vec::with_capacity(calls.len());
        for (index, child) in calls.into_iter().enumerate() {
            if let Some(cancel) = context
                .cancel
                .as_ref()
                .filter(|cancel| cancel.is_cancelled())
            {
                entries.push(json!({"index": index, "outcome": "not_attempted", "error": cancel.run_error()}));
                continue;
            }
            let result = super::team::async_helper_batch_item(
                context.clone(),
                agent.clone(),
                message.clone(),
                child,
            )
            .await;
            if result.status == TaskStatus::Error {
                entries.push(json!({
                    "index": index,
                    "outcome": "refused",
                    "error": clip(result.error.as_deref().unwrap_or("Native helper launch was refused"), 1024),
                    "errorTruncated": result.error.as_ref().is_some_and(|error| error.len() > 1024),
                }));
                continue;
            }
            let native = result
                .data
                .as_ref()
                .and_then(|content| serde_json::from_str::<Value>(&content.text).ok());
            let Some(native) = native.filter(|value| {
                ["runId", "agentId", "sessionId"].iter().all(|key| {
                    value[*key].as_str().is_some_and(|id| {
                        !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control)
                    })
                })
            }) else {
                // A successful launch with a malformed receipt is uncertain,
                // never a refusal that would invite a duplicate new operation.
                entries.push(json!({
                    "index": index,
                    "outcome": "receipt_unavailable",
                    "error": "Native launch returned no complete immutable handle; inspect helper runs before retrying",
                }));
                continue;
            };
            let mut handle = serde_json::Map::new();
            for key in [
                "runId",
                "agentId",
                "sessionId",
                "status",
                "completion",
                "bounds",
            ] {
                if let Some(value) = native.get(key) {
                    handle.insert(key.into(), value.clone());
                }
            }
            entries.push(json!({"index": index, "outcome": "accepted", "handle": handle}));
        }
        let accepted = entries
            .iter()
            .filter(|entry| entry["outcome"] == "accepted")
            .count();
        let refused = entries
            .iter()
            .filter(|entry| entry["outcome"] == "refused")
            .count();
        let unavailable = entries
            .iter()
            .filter(|entry| entry["outcome"] == "receipt_unavailable")
            .count();
        let not_attempted = entries
            .iter()
            .filter(|entry| entry["outcome"] == "not_attempted")
            .count();
        let truncated = entries.iter().any(|entry| entry["errorTruncated"] == true);
        let mut answer = json!({
            "atomic": false,
            "acceptedCount": accepted,
            "refusedCount": refused,
            "receiptUnavailableCount": unavailable,
            "notAttemptedCount": not_attempted,
            "entries": entries,
            "truncated": truncated,
        });
        loop {
            let text = answer.to_string();
            if text.len() <= MAX_BATCH_RESPONSE_BYTES {
                return TaskResult::success(
                    Content {
                        text,
                        ..Content::default()
                    },
                    0,
                );
            }
            let mut shortened = false;
            for entry in answer["entries"].as_array_mut().expect("batch entries") {
                if let Some(error) = entry["error"].as_str().filter(|error| !error.is_empty()) {
                    entry["error"] = json!(clip(error, error.len() / 2));
                    entry["errorTruncated"] = json!(true);
                    shortened = true;
                }
                if let Some(handle) = entry.get_mut("handle").and_then(Value::as_object_mut) {
                    // Identities and status are never removed to satisfy a cap.
                    for optional in ["completion", "bounds"] {
                        shortened |= handle.remove(optional).is_some();
                    }
                }
            }
            answer["truncated"] = json!(true);
            if !shortened {
                // Eight validated identities fit well below the response cap.
                // Fail closed if a future receipt shape violates that invariant.
                return TaskResult::error(
                    "Native helper identities exceed the batch receipt bound; inspect helper runs before retrying",
                    0,
                );
            }
        }
    })
}

pub(super) fn wait_helper_runs(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if !context.can_delegate {
            return TaskResult::error("Only an authorized coordinator run can collect helpers", 0);
        }
        let (Some(coordinator), Some(parent)) = (context.team, context.run_link) else {
            return TaskResult::error("Helper collection requires an authentic active run", 0);
        };
        match coordinator
            .collect_async_helpers(agent.id, parent, call, context.cancel)
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

fn clip(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
