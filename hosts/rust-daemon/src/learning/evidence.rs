use super::records::BRIEF_LIMIT;
use crate::app::SharedDaemonState;
use crate::history::HistoryMessage;
use crate::routes::ApiError;
use crate::runs::{ActionDurability, ActionStatus, RunSource, RunStatus};
use anima_core::{AgentConfig, MessageRole, ModelAdapter};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(super) struct CapturedSource {
    pub(super) session_id: String,
    pub(super) brief: String,
    pub(super) digest: String,
    pub(super) anchors: Vec<String>,
    pub(super) config: AgentConfig,
    pub(super) adapter: Arc<dyn ModelAdapter>,
}

pub(super) fn bounded_text(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Resolve only the exact source run and its accepted reply, never a transcript scan.
/// History I/O happens after releasing the state read guard.
pub(super) async fn capture_source(
    state: &SharedDaemonState,
    agent_id: &str,
    run_id: &str,
) -> Result<CapturedSource, ApiError> {
    let (mut run, config, adapter, store) = {
        let guard = state.read().await;
        let runtime = guard.agents.get(agent_id).ok_or_else(ApiError::not_found)?;
        if crate::agent_runs::is_helper_config(runtime.config()) {
            return Err(ApiError::conflict("Learning review source is not eligible"));
        }
        (
            guard.runs.get(run_id).cloned(),
            runtime.config().clone(),
            Arc::clone(&guard.model_adapter),
            guard.history.store(),
        )
    };
    if run.is_none() {
        run = store
            .get_run(run_id)
            .await
            .map_err(|_| ApiError::service_unavailable("Learning source history is unavailable"))?;
    }
    let run = run
        .filter(|run| run.agent_id == agent_id)
        .ok_or_else(ApiError::not_found)?;
    if run.status != RunStatus::Completed
        || run.error.is_some()
        || run.stop.is_some()
        || run.parent_run_id.is_some()
        || matches!(run.source, RunSource::Delegation | RunSource::Peer)
        || run.action_receipts.is_empty()
        || run.action_receipts.iter().any(|receipt| {
            receipt.status == ActionStatus::Started
                || receipt.durability != ActionDurability::Persistent
        })
    {
        return Err(ApiError::conflict(
            "Learning review requires a completed run with settled durable actions",
        ));
    }
    let reply_id = run
        .reply_message_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("Learning source has no accepted reply"))?;
    let hot = {
        let guard = state.read().await;
        if !guard.agents.contains_key(agent_id)
            || guard.sessions.get(agent_id, &run.session_id).is_none()
        {
            return Err(ApiError::not_found());
        }
        guard.agents.get(agent_id).and_then(|runtime| {
            runtime
                .messages()
                .iter()
                .find(|message| message.id == reply_id && message.room_id == run.session_id)
                .cloned()
        })
    };
    let reply = match hot {
        Some(message) => HistoryMessage {
            agent_id: agent_id.into(),
            session_id: run.session_id.clone(),
            hidden: false,
            message,
        },
        None => store
            .get_message(agent_id, &run.session_id, reply_id)
            .await
            .map_err(|_| ApiError::service_unavailable("Learning source history is unavailable"))?
            .ok_or_else(|| ApiError::conflict("Learning source reply is unavailable"))?,
    };
    let reply_run = reply
        .message
        .content
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("runId"));
    if reply.hidden
        || reply.message.role != MessageRole::Assistant
        || !matches!(reply_run, Some(anima_core::DataValue::String(id)) if id == run_id)
        || reply.message.content.text.trim().is_empty()
        || crate::schedules::is_silent_checkin_reply(&reply.message.content.text)
    {
        return Err(ApiError::conflict("Learning source reply is not eligible"));
    }
    let identity = json!({"runId":run.id,"agentId":run.agent_id,"sessionId":run.session_id,
        "input":run.input,"replyId":reply.message.id,"replyText":reply.message.content.text,
        "actions":run.action_receipts,"steps":run.steps,"model":run.model,"provider":run.provider});
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&identity).expect("JSON evidence serializes"))
    );
    let mut anchors = vec!["input".to_string(), "reply".to_string()];
    let mut actions = Vec::new();
    for receipt in run.action_receipts.iter().take(8) {
        if receipt.tool_call_id.len() > 256 || receipt.name.len() > 256 {
            return Err(ApiError::conflict(
                "Learning source action identity exceeds its bound",
            ));
        }
        let anchor = format!("receipt:{}", receipt.tool_call_id);
        anchors.push(anchor.clone());
        // Do not forward arbitrary argument/result bodies or attached file contents.
        let name = bounded_text(&receipt.name, 64);
        let summary = json!({"anchor":anchor,"name":name,"nameShortened":name.len()!=receipt.name.len(),
            "status":receipt.status,"durability":receipt.durability,"resultShortened":receipt.result_truncated,
            "resultContentOmitted":true});
        if summary.to_string().len() > 512 {
            return Err(ApiError::conflict(
                "Learning action summary exceeds its bound",
            ));
        }
        actions.push(summary);
    }
    let input = bounded_text(&run.input.text, 1536);
    let reply_text = bounded_text(&reply.message.content.text, 3072);
    let step_refs: Vec<&str> = run
        .steps
        .iter()
        .take(8)
        .filter(|step| step.step_id.len() <= 128)
        .map(|step| step.step_id.as_str())
        .collect();
    let mut brief = json!({"sourceRunId":run_id,"sessionId":run.session_id,
        "input":{"anchor":"input","text":input,"shortened":input.len()!=run.input.text.len()},
        "reply":{"anchor":"reply","messageId":reply_id,"text":reply_text,"shortened":reply_text.len()!=reply.message.content.text.len()},
        "actions":actions,"anchors":anchors,"receiptsShortened":run.action_receipts.len()>8,
        "stepRefs":step_refs,"stepRefsOmitted":step_refs.len()!=run.steps.len(),
        "meaning":"Completed means an accepted reply; reported action success is not whole-task proof."});
    let mut rendered = brief.to_string();
    while rendered.len() > BRIEF_LIMIT {
        let current = brief["reply"]["text"].as_str().unwrap_or("");
        if current.is_empty() {
            return Err(ApiError::conflict("Learning evidence exceeds its bound"));
        }
        brief["reply"]["text"] = json!(bounded_text(current, current.len().saturating_sub(256)));
        brief["reply"]["shortened"] = json!(true);
        rendered = brief.to_string();
    }
    Ok(CapturedSource {
        session_id: run.session_id,
        brief: rendered,
        digest,
        anchors,
        config,
        adapter,
    })
}
