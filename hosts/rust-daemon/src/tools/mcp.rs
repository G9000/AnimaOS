use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;
use serde_json::{Map, Number, Value};
use std::time::Instant;

const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
const MAX_ARGUMENT_DEPTH: usize = 16;
const MAX_ARGUMENT_NODES: usize = 1024;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub(super) fn execute(
    context: super::ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        if let Some(error) = missing_context_error(
            context.mcp.is_some(),
            context.mcp_stamp.is_some(),
            context.cancel.is_some(),
            context.run_link.is_some(),
        ) {
            return TaskResult::error(error, 0);
        }
        let manager = context.mcp.expect("checked MCP manager");
        let stamp = context.mcp_stamp.expect("checked MCP stamp");
        let cancel = context.cancel.expect("checked MCP cancellation context");
        let run_link = context.run_link.expect("checked MCP run context");

        if stamp.agent_id != agent.id
            || stamp.alias != call.name
            || stamp.tool_call_id != call.id
            || stamp.run_id != run_link.run_id
            || stamp.session_id != run_link.session_id
            || run_link.agent_id != agent.id
        {
            return TaskResult::error("MCP dispatch context changed", 0);
        }
        let args = match arguments(&call) {
            Ok(args) => args,
            Err(error) => return TaskResult::error(error, 0),
        };

        let started = Instant::now();
        match manager.invoke(stamp, args, cancel).await {
            Ok(content) if external_error_reported(&content) => anima_core::TaskResult {
                status: anima_core::TaskStatus::Error,
                data: Some(content),
                error: Some("External tool reported failure".into()),
                duration_ms: elapsed_ms(started),
            },
            Ok(content) => TaskResult::success(content, elapsed_ms(started)),
            Err(error) => TaskResult::error(error.to_string(), elapsed_ms(started)),
        }
    })
}

fn external_error_reported(content: &Content) -> bool {
    serde_json::from_str::<Value>(&content.text)
        .ok()
        .and_then(|value| value.get("isError").and_then(Value::as_bool))
        == Some(true)
}

/// Converts the core's tagged `DataValue` tree to ordinary JSON after a cheap
/// reference-only validation pass. This keeps oversized or unsafe input from
/// being cloned into a transport payload.
pub(super) fn arguments(call: &ToolCall) -> Result<Value, &'static str> {
    let mut limits = Limits {
        bytes: 2, // Root object delimiters.
        nodes: 1, // Root object.
    };
    for (index, (key, value)) in call.args.iter().enumerate() {
        if index > 0 {
            limits.add_bytes(1)?;
        }
        limits.add_bytes(json_string_len(key)?)?;
        limits.add_bytes(1)?; // Colon.
        limits.check_node(2)?;
        limits.value(value, 2)?;
    }

    Ok(Value::Object(
        call.args
            .iter()
            .map(|(key, value)| Ok((key.clone(), to_json(value)?)))
            .collect::<Result<Map<_, _>, &'static str>>()?,
    ))
}

struct Limits {
    bytes: usize,
    nodes: usize,
}

impl Limits {
    fn add_bytes(&mut self, amount: usize) -> Result<(), &'static str> {
        self.bytes = self.bytes.checked_add(amount).ok_or(ARGUMENT_LIMIT)?;
        if self.bytes > MAX_ARGUMENT_BYTES {
            return Err(ARGUMENT_LIMIT);
        }
        Ok(())
    }

    fn check_node(&mut self, depth: usize) -> Result<(), &'static str> {
        if depth > MAX_ARGUMENT_DEPTH {
            return Err(ARGUMENT_LIMIT);
        }
        self.nodes = self.nodes.checked_add(1).ok_or(ARGUMENT_LIMIT)?;
        if self.nodes > MAX_ARGUMENT_NODES {
            return Err(ARGUMENT_LIMIT);
        }
        Ok(())
    }

    fn value(&mut self, value: &DataValue, depth: usize) -> Result<(), &'static str> {
        match value {
            DataValue::Null | DataValue::Bool(_) => {
                self.add_bytes(if matches!(value, DataValue::Null) {
                    4
                } else {
                    5
                })
            }
            DataValue::Number(value) => {
                if !value.is_finite() || (value.fract() == 0.0 && value.abs() > MAX_SAFE_INTEGER) {
                    return Err(ARGUMENT_LIMIT);
                }
                let number = Number::from_f64(*value).ok_or(ARGUMENT_LIMIT)?;
                self.add_bytes(number.to_string().len())
            }
            DataValue::String(value) => self.add_bytes(json_string_len(value)?),
            DataValue::Array(values) => {
                self.add_bytes(2)?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        self.add_bytes(1)?;
                    }
                    self.check_node(depth + 1)?;
                    self.value(value, depth + 1)?;
                }
                Ok(())
            }
            DataValue::Object(values) => {
                self.add_bytes(2)?;
                for (index, (key, value)) in values.iter().enumerate() {
                    if index > 0 {
                        self.add_bytes(1)?;
                    }
                    self.add_bytes(json_string_len(key)?)?;
                    self.add_bytes(1)?;
                    self.check_node(depth + 1)?;
                    self.value(value, depth + 1)?;
                }
                Ok(())
            }
        }
    }
}

const ARGUMENT_LIMIT: &str = "MCP arguments exceed safe bounds";

fn json_string_len(value: &str) -> Result<usize, &'static str> {
    let escaped = value.chars().try_fold(2usize, |length, character| {
        let encoded = match character {
            '"' | '\\' | '\u{08}' | '\u{09}' | '\u{0a}' | '\u{0c}' | '\u{0d}' => 2,
            '\u{00}'..='\u{1f}' => 6,
            _ => character.len_utf8(),
        };
        length.checked_add(encoded)
    });
    escaped.ok_or(ARGUMENT_LIMIT)
}

fn to_json(value: &DataValue) -> Result<Value, &'static str> {
    Ok(match value {
        DataValue::Null => Value::Null,
        DataValue::Bool(value) => Value::Bool(*value),
        DataValue::Number(value) => Value::Number(Number::from_f64(*value).ok_or(ARGUMENT_LIMIT)?),
        DataValue::String(value) => Value::String(value.clone()),
        DataValue::Array(values) => {
            Value::Array(values.iter().map(to_json).collect::<Result<Vec<_>, _>>()?)
        }
        DataValue::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), to_json(value)?)))
                .collect::<Result<Map<_, _>, &'static str>>()?,
        ),
    })
}

fn missing_context_error(
    manager: bool,
    stamp: bool,
    cancel: bool,
    run: bool,
) -> Option<&'static str> {
    if !manager {
        Some("MCP dispatch context unavailable")
    } else if !stamp {
        Some("MCP dispatch stamp unavailable")
    } else if !cancel {
        Some("MCP cancellation context unavailable")
    } else if !run {
        Some("MCP run context unavailable")
    } else {
        None
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::{arguments, execute};
    use crate::memory_embeddings::MemoryEmbeddingRuntime;
    use crate::tools::{
        new_shared_process_manager_with_limit, ToolExecutionContext, ToolRegistry,
        DEFAULT_MAX_BACKGROUND_PROCESSES,
    };
    use anima_core::{
        AgentConfig, AgentState, AgentStatus, CancelSignal, Content, DataValue, Message,
        MessageRole, ToolCall,
    };
    use anima_memory::MemoryManager;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use tokio::sync::RwLock as AsyncRwLock;

    fn call(args: BTreeMap<String, DataValue>) -> ToolCall {
        ToolCall {
            id: "call-1".into(),
            name: "mcp_fixture".into(),
            args,
        }
    }

    #[test]
    fn converts_data_values_to_ordinary_nested_json() {
        let value = arguments(&call(BTreeMap::from([
            ("enabled".into(), DataValue::Bool(true)),
            (
                "input".into(),
                DataValue::Object(BTreeMap::from([
                    ("count".into(), DataValue::Number(3.0)),
                    (
                        "items".into(),
                        DataValue::Array(vec![DataValue::String("one".into()), DataValue::Null]),
                    ),
                ])),
            ),
        ])))
        .expect("bounded arguments convert");

        assert_eq!(
            value,
            json!({"enabled": true, "input": {"count": 3.0, "items": ["one", null]}})
        );
        assert!(
            value.get("Object").is_none(),
            "DataValue variants are not wire tags"
        );
    }

    #[test]
    fn rejects_unsafe_numbers_excessive_depth_nodes_and_bytes() {
        assert!(arguments(&call(BTreeMap::from([(
            "nan".into(),
            DataValue::Number(f64::NAN),
        )])))
        .is_err());
        assert!(arguments(&call(BTreeMap::from([(
            "integer".into(),
            DataValue::Number(9_007_199_254_740_992.0),
        )])))
        .is_err());

        let mut deep = DataValue::Null;
        for _ in 0..16 {
            deep = DataValue::Array(vec![deep]);
        }
        assert!(arguments(&call(BTreeMap::from([("deep".into(), deep)]))).is_err());

        let too_many = DataValue::Array(vec![DataValue::Null; 1024]);
        assert!(arguments(&call(BTreeMap::from([("many".into(), too_many)]))).is_err());

        let too_large = DataValue::String("x".repeat(16 * 1024));
        assert!(arguments(&call(BTreeMap::from([("large".into(), too_large)]))).is_err());
    }

    #[test]
    fn rejects_missing_dispatch_context_before_any_transport_call() {
        assert_eq!(
            super::missing_context_error(false, false, false, false),
            Some("MCP dispatch context unavailable")
        );
        assert_eq!(
            super::missing_context_error(true, false, true, true),
            Some("MCP dispatch stamp unavailable")
        );
        assert_eq!(
            super::missing_context_error(true, true, false, true),
            Some("MCP cancellation context unavailable")
        );
        assert_eq!(
            super::missing_context_error(true, true, true, false),
            Some("MCP run context unavailable")
        );
        assert_eq!(super::missing_context_error(true, true, true, true), None);
    }

    #[tokio::test]
    async fn executor_returns_static_error_when_mcp_context_is_absent() {
        let context = ToolExecutionContext::new(
            Arc::new(AsyncRwLock::new(MemoryManager::new())),
            Arc::new(AsyncRwLock::new(MemoryEmbeddingRuntime::disabled())),
            None,
            ToolRegistry::new(),
            new_shared_process_manager_with_limit(DEFAULT_MAX_BACKGROUND_PROCESSES),
            None,
            None,
        )
        .with_run_link(Some(crate::runs::RunLink {
            run_id: "run-1".into(),
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
        }))
        .with_cancel(Some(CancelSignal::new()));
        let agent = AgentState {
            id: "agent-1".into(),
            name: "agent".into(),
            status: AgentStatus::Running,
            config: AgentConfig {
                name: "agent".into(),
                model: "deterministic".into(),
                bio: None,
                lore: None,
                knowledge: None,
                topics: None,
                adjectives: None,
                style: None,
                provider: None,
                system: None,
                tools: None,
                plugins: None,
                settings: None,
                fallbacks: Vec::new(),
            },
            created_at_ms: 1,
            token_usage: Default::default(),
        };
        let message = Message {
            id: "message-1".into(),
            agent_id: agent.id.clone(),
            room_id: "room-1".into(),
            content: Content::default(),
            role: MessageRole::User,
            created_at_ms: 1,
        };
        let result = execute(context, agent, message, call(BTreeMap::new())).await;

        assert_eq!(result.status, anima_core::TaskStatus::Error);
        assert_eq!(
            result.error.as_deref(),
            Some("MCP dispatch context unavailable")
        );
        assert_eq!(result.duration_ms, 0);
    }
}
