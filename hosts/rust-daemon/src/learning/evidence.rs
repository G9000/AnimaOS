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
    /// Settled tool calls in the source run, before the brief's bound.
    pub(super) action_count: usize,
    pub(super) skill_signal: bool,
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
            guard.profile_model_adapter(),
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
        || (run.action_receipts.is_empty()
            && !super::skill_evidence::correction_signal(&run.effective_input().text))
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
        "input":run.effective_input(),"replyId":reply.message.id,"replyText":reply.message.content.text,
        "actions":run.action_receipts,"steps":run.steps,"model":run.model,"provider":run.provider});
    let digest = hex::encode(Sha256::digest(
        serde_json::to_vec(&identity).expect("JSON evidence serializes"),
    ));
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
    let input =
        anima_core::skill_learning::safe_learning_excerpt(&run.effective_input().text, 1536);
    let reply_text =
        anima_core::skill_learning::safe_learning_excerpt(&reply.message.content.text, 3072);
    let (skill_learning_allowed, skill_signal, skills) =
        super::skill_evidence::context(state, &run).await;
    let procedure_evidence: Vec<_> = run
        .action_receipts
        .iter()
        .take(8)
        .filter_map(super::skill_evidence::procedure_evidence)
        .collect();
    let step_refs: Vec<&str> = run
        .steps
        .iter()
        .take(8)
        .filter(|step| step.step_id.len() <= 128)
        .map(|step| step.step_id.as_str())
        .collect();
    let mut brief = json!({"sourceRunId":run_id,"sessionId":run.session_id,
        "skillLearningAllowed":skill_learning_allowed,"skills":skills,"procedureEvidence":procedure_evidence,
        "input":{"anchor":"input","text":input,"shortened":input.len()!=run.effective_input().text.len()},
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
        action_count: run.action_receipts.len(),
        skill_signal,
        session_id: run.session_id,
        brief: rendered,
        digest,
        anchors,
        config,
        adapter,
    })
}

/// The anchor a conversation brief gives one turn.
pub(super) fn turn_anchor(run_id: &str) -> String {
    format!("turn:{run_id}")
}

/// The run ids a conversation review's anchors name, oldest first.
pub(super) fn anchored_turns(anchors: &[String]) -> Vec<String> {
    anchors
        .iter()
        .filter_map(|anchor| anchor.strip_prefix("turn:"))
        .map(str::to_owned)
        .collect()
}

/// Bytes each owner message and reply starts with in a conversation brief;
/// both shrink further while the brief is over `BRIEF_LIMIT`.
const TURN_TEXT_LIMIT: usize = 900;

/// One counted turn: the owner's message and the agent's accepted reply.
struct Turn {
    run_id: String,
    session_id: String,
    owner: String,
    reply_id: String,
    reply: String,
}

/// A conversation review's evidence: the agent's last counted owner turns
/// (`run_ids`, oldest first, ending with `source_run_id`) and its current core
/// memory. A turn whose run or reply is gone is left out. The digest covers
/// only the turns, so a core edit between admission and dispatch does not
/// cancel the review; the blocks in the brief are those at admission.
pub(super) async fn capture_conversation(
    state: &SharedDaemonState,
    agent_id: &str,
    source_run_id: &str,
    run_ids: &[String],
) -> Result<CapturedSource, ApiError> {
    let (config, adapter, store, memory, hot_runs) = {
        let guard = state.read().await;
        let runtime = guard.agents.get(agent_id).ok_or_else(ApiError::not_found)?;
        if crate::agent_runs::is_helper_config(runtime.config()) {
            return Err(ApiError::conflict("Learning review source is not eligible"));
        }
        let hot_runs: Vec<_> = run_ids
            .iter()
            .map(|id| guard.runs.get(id).cloned())
            .collect();
        (
            runtime.config().clone(),
            guard.profile_model_adapter(),
            guard.history.store(),
            guard.memory_handle(),
            hot_runs,
        )
    };
    let mut turns = Vec::new();
    for (run_id, hot) in run_ids.iter().zip(hot_runs) {
        let run = match hot {
            Some(run) => Some(run),
            None => store.get_run(run_id).await.map_err(|_| {
                ApiError::service_unavailable("Learning source history is unavailable")
            })?,
        };
        let Some(run) = run.filter(|run| {
            run.agent_id == agent_id
                && run.status == RunStatus::Completed
                && run.error.is_none()
                && run.stop.is_none()
        }) else {
            continue;
        };
        let Some(reply_id) = run.reply_message_id.clone() else {
            continue;
        };
        let hot = {
            let guard = state.read().await;
            guard.agents.get(agent_id).and_then(|runtime| {
                runtime
                    .messages()
                    .iter()
                    .find(|message| message.id == reply_id && message.room_id == run.session_id)
                    .cloned()
            })
        };
        let reply = match hot {
            Some(message) => Some((false, message)),
            None => store
                .get_message(agent_id, &run.session_id, &reply_id)
                .await
                .map_err(|_| {
                    ApiError::service_unavailable("Learning source history is unavailable")
                })?
                .map(|stored| (stored.hidden, stored.message)),
        };
        let Some((false, reply)) = reply else {
            continue;
        };
        if reply.role != MessageRole::Assistant || reply.content.text.trim().is_empty() {
            continue;
        }
        let owner_text = run.effective_input().text.clone();
        turns.push(Turn {
            run_id: run.id,
            session_id: run.session_id,
            owner: owner_text,
            reply_id,
            reply: reply.content.text,
        });
    }
    let session_id = turns
        .iter()
        .find(|turn| turn.run_id == source_run_id)
        .map(|turn| turn.session_id.clone())
        .ok_or_else(|| ApiError::conflict("Learning source reply is not eligible"))?;
    let identity: Vec<_> = turns
        .iter()
        .map(|turn| {
            json!({"runId":turn.run_id,"sessionId":turn.session_id,"input":turn.owner,
                "replyId":turn.reply_id,"replyText":turn.reply})
        })
        .collect();
    let digest = hex::encode(Sha256::digest(
        serde_json::to_vec(&json!({"agentId":agent_id,"turns":identity}))
            .expect("JSON evidence serializes"),
    ));
    let core = {
        let manager = memory.read().await;
        let (user, notes) = crate::core_memory::blocks_for(&manager, agent_id);
        format!(
            "{}{}",
            crate::core_memory::render_block(crate::core_memory::CoreBlock::User, &user),
            crate::core_memory::render_block(crate::core_memory::CoreBlock::Notes, &notes)
        )
    };
    let anchors: Vec<String> = turns.iter().map(|turn| turn_anchor(&turn.run_id)).collect();
    let mut brief = json!({"kind":"conversation","sourceRunId":source_run_id,
        "turns":turns.iter().map(|turn| {
            let owner = bounded_text(&turn.owner, TURN_TEXT_LIMIT);
            let reply = bounded_text(&turn.reply, TURN_TEXT_LIMIT);
            json!({"anchor":turn_anchor(&turn.run_id),
                "owner":{"text":owner,"shortened":owner.len()!=turn.owner.len()},
                "reply":{"text":reply,"shortened":reply.len()!=turn.reply.len()}})
        }).collect::<Vec<_>>(),
        "coreMemory":core,"anchors":anchors,
        "meaning":"The owner's recent messages and the agent's replies, oldest first, then the current core memory blocks."});
    let mut rendered = brief.to_string();
    while rendered.len() > BRIEF_LIMIT {
        // Shorten the longest message first; the blocks only when no turn
        // text is left to cut.
        let longest = brief["turns"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .flat_map(|(index, turn)| {
                ["owner", "reply"].map(|side| {
                    let length = turn[side]["text"].as_str().map_or(0, str::len);
                    (length, index, side)
                })
            })
            .max();
        match longest {
            Some((length, index, side)) if length > 0 => {
                let text = brief["turns"][index][side]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                brief["turns"][index][side]["text"] =
                    json!(bounded_text(&text, length.saturating_sub(128)));
                brief["turns"][index][side]["shortened"] = json!(true);
            }
            _ => {
                let current = brief["coreMemory"].as_str().unwrap_or_default().to_owned();
                if current.is_empty() {
                    return Err(ApiError::conflict("Learning evidence exceeds its bound"));
                }
                brief["coreMemory"] =
                    json!(bounded_text(&current, current.len().saturating_sub(256)));
            }
        }
        rendered = brief.to_string();
    }
    Ok(CapturedSource {
        action_count: 0,
        skill_signal: false,
        session_id,
        brief: rendered,
        digest,
        anchors,
        config,
        adapter,
    })
}
