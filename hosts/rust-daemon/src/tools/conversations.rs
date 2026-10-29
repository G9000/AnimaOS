//! Bounded discovery and reading of an agent's own conversation history.

use std::collections::BTreeSet;

use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;

use super::ToolExecutionContext;
use crate::sessions::{
    history_read::{BrowseCursor, DiscoveryRequest, WindowRequest},
    SessionKind,
};

const DEFAULT_RESULTS: usize = 5;
pub(super) const MAX_RESULTS: usize = 10;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAX_ID_BYTES: usize = 256;

fn invalid(message: &'static str) -> TaskResult<Content> {
    TaskResult::error(message, 0)
}

fn unknown_keys(call: &ToolCall, allowed: &[&str]) -> bool {
    let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
    call.args.keys().any(|key| !allowed.contains(key.as_str()))
}

fn string_arg<'a>(call: &'a ToolCall, key: &str) -> Option<&'a str> {
    match call.args.get(key) {
        Some(DataValue::String(value)) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

fn bounded_integer(call: &ToolCall, key: &str, default: u64, max: u64) -> Option<u64> {
    match call.args.get(key) {
        None => Some(default),
        Some(DataValue::Number(value))
            if value.is_finite() && value.fract() == 0.0 && (0.0..=max as f64).contains(value) =>
        {
            Some(*value as u64)
        }
        Some(_) => None,
    }
}

fn json_result(value: serde_json::Value) -> TaskResult<Content> {
    TaskResult::success(
        Content {
            text: value.to_string(),
            ..Content::default()
        },
        0,
    )
}

fn discovery_request(call: &ToolCall) -> Result<DiscoveryRequest, &'static str> {
    if unknown_keys(
        call,
        &["query", "limit", "kind", "after_ms", "before_ms", "cursor"],
    ) {
        return Err("search_conversations received an unknown argument");
    }
    let query = match call.args.get("query") {
        None => "",
        Some(DataValue::String(query)) if !query.trim().is_empty() => query,
        Some(_) => return Err("search_conversations query must be a non-empty string"),
    };
    if query.trim().chars().count() > 200 {
        return Err("search_conversations query must be at most 200 characters");
    }
    let limit = bounded_integer(call, "limit", DEFAULT_RESULTS as u64, MAX_RESULTS as u64)
        .filter(|value| *value >= 1)
        .ok_or("search_conversations limit must be an integer from 1 to 10")?
        as usize;
    let kind = match call.args.get("kind") {
        None => None,
        Some(DataValue::String(value)) => Some(match value.as_str() {
            "chat" => SessionKind::Chat,
            "telegram" => SessionKind::Telegram,
            "discord" => SessionKind::Discord,
            "email" => SessionKind::Email,
            "checkin" => SessionKind::Checkin,
            "job" => SessionKind::Job,
            "helper" => SessionKind::Helper,
            _ => return Err(
                "search_conversations kind must be chat, telegram, discord, email, checkin, job, or helper (own incoming agent conversations)",
            ),
        }),
        Some(_) => {
            return Err(
                "search_conversations kind must be chat, telegram, discord, email, checkin, job, or helper (own incoming agent conversations)",
            )
        }
    };
    let after_ms = bounded_integer(call, "after_ms", 0, MAX_SAFE_INTEGER as u64)
        .filter(|_| call.args.contains_key("after_ms"));
    if call.args.contains_key("after_ms") && after_ms.is_none() {
        return Err("search_conversations after_ms must be a safe non-negative integer");
    }
    let before_ms = bounded_integer(call, "before_ms", 0, MAX_SAFE_INTEGER as u64)
        .filter(|_| call.args.contains_key("before_ms"));
    if call.args.contains_key("before_ms") && before_ms.is_none() {
        return Err("search_conversations before_ms must be a safe non-negative integer");
    }
    if matches!((after_ms, before_ms), (Some(after), Some(before)) if after >= before) {
        return Err("search_conversations after_ms must be earlier than before_ms");
    }
    Ok(DiscoveryRequest {
        query: query.trim().to_string(),
        limit,
        kind,
        after_ms,
        before_ms,
    })
}

pub(super) fn search_conversations(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let request = match discovery_request(&call) {
            Ok(request) => request,
            Err(message) => return invalid(message),
        };
        let Some(coordinator) = context.team.clone() else {
            return invalid("Conversation search is unavailable in this execution context");
        };
        let current = context
            .run_link
            .as_ref()
            .map(|link| link.session_id.clone());
        let cursor = match call.args.get("cursor") {
            None => None,
            Some(DataValue::String(value)) if request.query.is_empty() => {
                match BrowseCursor::parse(value) {
                    Some(cursor) => Some(cursor),
                    None => {
                        return invalid(
                            "search_conversations cursor must be a returned browse cursor",
                        )
                    }
                }
            }
            Some(_) => {
                return invalid(
                    "search_conversations cursor is only valid when browsing without query",
                )
            }
        };
        let lookup = async {
            let mut value = if request.query.is_empty() {
                coordinator
                    .browse_conversations(&agent.id, current.as_deref(), request, cursor)
                    .await?
            } else {
                coordinator
                    .discover_conversations(&agent.id, current.as_deref(), request)
                    .await?
            };
            if can_read_context(&context, &agent).await {
                coordinator
                    .hydrate_conversation_result(&agent.id, &mut value)
                    .await?;
                if !can_read_context(&context, &agent).await {
                    if let Some(first) = value["results"]
                        .as_array_mut()
                        .and_then(|rows| rows.first_mut())
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        first.remove("context");
                    }
                }
            }
            coordinator
                .finalize_conversation_result(&agent.id, &mut value, |state| {
                    can_read_context_current(&context, &agent, state)
                })
                .await;
            Ok::<_, crate::sessions::history_read::HistoryReadError>(value)
        };
        match tokio::time::timeout(std::time::Duration::from_secs(5), lookup).await {
            Ok(Ok(value)) => json_result(value),
            Ok(Err(error)) => TaskResult::error(error.to_string(), 0),
            Err(_) => invalid("History lookup is unavailable or exceeded its result deadline"),
        }
    })
}

enum ReadTarget {
    Latest {
        session_id: String,
        before_count: usize,
    },
    Around(WindowRequest),
}

fn can_read_context_current(
    context: &ToolExecutionContext,
    agent: &AgentState,
    state: &crate::state::DaemonState,
) -> bool {
    if let (Some(catalog), Some(run)) = (&context.tool_catalog, &context.run_link) {
        return catalog
            .check_current(state, agent, run, "read_conversation_window")
            .is_ok();
    }
    context.delegated_parent.is_none()
        && context.peer_sources.is_empty()
        && agent.config.allows_tool("read_conversation_window")
        && state
            .agents
            .get(&agent.id)
            .is_some_and(|runtime| runtime.config().allows_tool("read_conversation_window"))
}

async fn can_read_context(context: &ToolExecutionContext, agent: &AgentState) -> bool {
    if let (Some(catalog), Some(run)) = (&context.tool_catalog, &context.run_link) {
        return catalog
            .check(agent, run, "read_conversation_window")
            .await
            .is_ok();
    }
    if !agent.config.allows_tool("read_conversation_window") {
        return false;
    }
    let Some(coordinator) = &context.team else {
        return false;
    };
    if !coordinator
        .peer_allows_tool(&agent.id, "read_conversation_window")
        .await
    {
        return false;
    }
    if let Some(parent) = &context.delegated_parent {
        if !coordinator
            .parent_allows_tool(parent, "read_conversation_window")
            .await
        {
            return false;
        }
    }
    for source in &context.peer_sources {
        if !coordinator
            .peer_allows_tool(source, "read_conversation_window")
            .await
        {
            return false;
        }
    }
    true
}

fn window_request(call: &ToolCall) -> Result<ReadTarget, &'static str> {
    if unknown_keys(
        call,
        &[
            "session_id",
            "anchor_message_id",
            "before_count",
            "after_count",
        ],
    ) {
        return Err("read_conversation_window received an unknown argument");
    }
    let session_id = string_arg(call, "session_id")
        .filter(|value| value.len() <= MAX_ID_BYTES)
        .ok_or("read_conversation_window session_id must be a non-empty string up to 256 bytes")?;
    let anchor_message_id = match call.args.get("anchor_message_id") {
        None => None,
        Some(_) => Some(string_arg(call,"anchor_message_id").filter(|value|value.len()<=MAX_ID_BYTES)
            .ok_or("read_conversation_window anchor_message_id must be a non-empty string up to 256 bytes")?),
    };
    let before_count = bounded_integer(
        call,
        "before_count",
        if anchor_message_id.is_some() { 5 } else { 19 },
        19,
    )
    .ok_or("read_conversation_window before_count must be an integer from 0 to 19")?
        as usize;
    let after_count = bounded_integer(
        call,
        "after_count",
        if anchor_message_id.is_some() { 5 } else { 0 },
        19,
    )
    .ok_or("read_conversation_window after_count must be an integer from 0 to 19")?
        as usize;
    if before_count + after_count > 19 {
        return Err("read_conversation_window before_count plus after_count must be at most 19");
    }
    let Some(anchor_message_id) = anchor_message_id else {
        if after_count != 0 {
            return Err("read_conversation_window after_count requires an anchor_message_id");
        }
        return Ok(ReadTarget::Latest {
            session_id: session_id.into(),
            before_count,
        });
    };
    Ok(ReadTarget::Around(WindowRequest {
        session_id: session_id.to_string(),
        anchor_message_id: anchor_message_id.to_string(),
        before_count,
        after_count,
    }))
}

pub(super) fn read_conversation_window(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let request = match window_request(&call) {
            Ok(request) => request,
            Err(message) => return invalid(message),
        };
        let Some(coordinator) = context.team.clone() else {
            return invalid("Conversation history is unavailable in this execution context");
        };
        let result = match request {
            ReadTarget::Latest {
                session_id,
                before_count,
            } => {
                coordinator
                    .read_latest_conversation(&agent.id, &session_id, before_count)
                    .await
            }
            ReadTarget::Around(request) => {
                coordinator
                    .read_conversation_window(&agent.id, request)
                    .await
            }
        };
        match result {
            Ok(value) => json_result(value),
            Err(error) => TaskResult::error(error.to_string(), 0),
        }
    })
}
