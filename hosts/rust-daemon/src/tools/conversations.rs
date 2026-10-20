//! Bounded discovery and reading of an agent's own conversation history.

use std::collections::BTreeSet;

use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;

use super::ToolExecutionContext;
use crate::sessions::{
    history_read::{DiscoveryRequest, WindowRequest},
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
    if unknown_keys(call, &["query", "limit", "kind", "after_ms", "before_ms"]) {
        return Err("search_conversations received an unknown argument");
    }
    let query =
        string_arg(call, "query").ok_or("search_conversations query must be a non-empty string")?;
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
            _ => return Err(
                "search_conversations kind must be chat, telegram, discord, email, checkin, or job",
            ),
        }),
        Some(_) => {
            return Err(
                "search_conversations kind must be chat, telegram, discord, email, checkin, or job",
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
        match coordinator
            .discover_conversations(&agent.id, current.as_deref(), request)
            .await
        {
            Ok(value) => json_result(value),
            Err(error) => TaskResult::error(error.to_string(), 0),
        }
    })
}

fn window_request(call: &ToolCall) -> Result<WindowRequest, &'static str> {
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
    let anchor_message_id = string_arg(call, "anchor_message_id")
        .filter(|value| value.len() <= MAX_ID_BYTES)
        .ok_or(
            "read_conversation_window anchor_message_id must be a non-empty string up to 256 bytes",
        )?;
    let before_count = bounded_integer(call, "before_count", 5, 19)
        .ok_or("read_conversation_window before_count must be an integer from 0 to 19")?
        as usize;
    let after_count = bounded_integer(call, "after_count", 5, 19)
        .ok_or("read_conversation_window after_count must be an integer from 0 to 19")?
        as usize;
    if before_count + after_count > 19 {
        return Err("read_conversation_window before_count plus after_count must be at most 19");
    }
    Ok(WindowRequest {
        session_id: session_id.to_string(),
        anchor_message_id: anchor_message_id.to_string(),
        before_count,
        after_count,
    })
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
        match coordinator
            .read_conversation_window(&agent.id, request)
            .await
        {
            Ok(value) => json_result(value),
            Err(error) => TaskResult::error(error.to_string(), 0),
        }
    })
}
