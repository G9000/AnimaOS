//! Recent-session browsing and reading share the exact history ownership policy.

use std::collections::HashSet;

use anima_core::{Message, MessageRole};
use serde_json::{json, Value};

use super::{
    allowed, bound_window_to, clip, format_time, hidden_message_ids, missing, pairing_incomplete,
    read_conversation_window, row_allowed, shape_message, size, valid_id, DiscoveryRequest,
    HistoryReadError, MessageOrder, MessagePageQuery, SharedDaemonState, WindowRequest, DEADLINE,
    DISCOVERY_BYTES, GUIDANCE,
};
use crate::history::{
    display_text, role_name, search_snippet, search_tokens, searchable_text, snippet_text,
    text_matches,
};

#[derive(Clone, Debug)]
pub(crate) struct BrowseCursor {
    pub(crate) last_activity_at_ms: u64,
    pub(crate) session_id: String,
}

impl BrowseCursor {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.len() > 512 {
            return None;
        }
        let (time, id) = value.split_once(':')?;
        let last_activity_at_ms = time.parse().ok()?;
        (last_activity_at_ms <= 9_007_199_254_740_991 && valid_id(id)).then(|| Self {
            last_activity_at_ms,
            session_id: id.into(),
        })
    }
}

pub(crate) async fn browse_conversations(
    state: &SharedDaemonState,
    agent: &str,
    current: Option<&str>,
    request: DiscoveryRequest,
    cursor: Option<BrowseCursor>,
) -> Result<Value, HistoryReadError> {
    if !request.query.is_empty()
        || !(1..=10).contains(&request.limit)
        || request
            .after_ms
            .is_some_and(|time| time > 9_007_199_254_740_991)
        || request
            .before_ms
            .is_some_and(|time| time > 9_007_199_254_740_991)
        || matches!((request.after_ms,request.before_ms),(Some(after),Some(before)) if after>=before)
        || cursor.as_ref().is_some_and(|cursor| {
            !valid_id(&cursor.session_id) || cursor.last_activity_at_ms > 9_007_199_254_740_991
        })
    {
        return Err(HistoryReadError::InvalidRequest);
    }
    tokio::time::timeout(DEADLINE, async {
        let mut records = {
            let guard = state.read().await;
            guard.sessions.records().filter(|record| {
                allowed(&guard, agent, record)
                    && Some(record.id.as_str()) != current
                    && request.kind.is_none_or(|kind| record.kind == kind)
                    && request.after_ms.is_none_or(|time| record.last_activity_at_ms >= time)
                    && request.before_ms.is_none_or(|time| record.last_activity_at_ms < time)
                    && cursor.as_ref().is_none_or(|cursor| {
                        (record.last_activity_at_ms, record.id.as_str())
                            < (cursor.last_activity_at_ms, cursor.session_id.as_str())
                    })
            }).cloned().collect::<Vec<_>>()
        };
        if records.len() > 500 {
            return Err(HistoryReadError::ScopeLimit);
        }
        records.sort_by(|a, b| b.last_activity_at_ms.cmp(&a.last_activity_at_ms).then_with(|| b.id.cmp(&a.id)));
        let has_more = records.len() > request.limit;
        records.truncate(request.limit);
        let last_scanned=records.last().map(|record|format!("{}:{}",record.last_activity_at_ms,record.id));
        let mut results = Vec::new();
        for record in records {
            let latest = newest_message(state, agent, &record.id).await?;
            let guard = state.read().await;
            let Some(live) = guard.sessions.get(agent, &record.id).filter(|live| {
                allowed(&guard, agent, live) && live.room_id() == record.room_id()
            }) else { continue };
            let live_hidden=hidden_message_ids(guard.agents[agent].messages().iter().filter(|message|message.room_id==live.room_id()));
            let latest=latest.filter(|message|!live_hidden.contains(&message.id));
            let preview = latest.as_ref().map(|message| search_snippet(snippet_text(message), &[]))
                .unwrap_or_else(|| clip(&record.title, 240));
            results.push(json!({
                "sessionId":record.id,"title":clip(&record.title,240),"kind":record.kind.as_str(),
                "archived":record.archived,"createdAtMs":record.created_at_ms,
                "createdAt":format_time(record.created_at_ms),"lastActivityAtMs":record.last_activity_at_ms,
                "lastActivityAt":format_time(record.last_activity_at_ms),
                "matchMessageId":latest.as_ref().map(|message| &message.id),
                "matchedAtMs":latest.as_ref().map(|message| message.created_at_ms),
                "matchedAt":latest.as_ref().map(|message| format_time(message.created_at_ms)),
                "role":latest.as_ref().map(|message| role_name(message.role)),
                "snippet":preview,"snippetShortened":latest.as_ref().is_some_and(|message| display_text(message)!=preview),
                "titleShortened":record.title.len()>240,"matchKind":"recent",
                "anchorUnavailable":latest.is_none(),"contact":crate::sessions::interactions::contact(&guard,live),
            }));
        }
        let mut output = json!({"guidance":GUIDANCE,"mode":"browse",
            "status":if results.is_empty(){"not_found"}else{"found"},"results":results,
            "hasMore":has_more,"shortened":false,"nextCursor":if has_more{json!(last_scanned)}else{Value::Null},
            "hint":"Open a session with read_conversation_window(session_id=...). Read the recorded exchange before answering. A search miss does not prove no discussion occurred."});
        loop {
            if output["hasMore"] == true {
                output["nextCursor"] = output["results"].as_array().and_then(|rows|rows.last()).map(|last|
                    json!(format!("{}:{}", last["lastActivityAtMs"].as_u64().expect("activity time"), last["sessionId"].as_str().expect("session ID")))
                ).unwrap_or_else(||json!(last_scanned));
            }
            if size(&output)<=DISCOVERY_BYTES {break}
            if output["results"].as_array_mut().expect("browse results").pop().is_none() {break}
            output["shortened"] = json!(true);
            output["hasMore"] = json!(true);
        }
        finalize_discovery(state,agent,&mut output).await;
        Ok(output)
    }).await.map_err(|_| HistoryReadError::Unavailable)?
}

pub(crate) async fn finalize_discovery(state: &SharedDaemonState, agent: &str, output: &mut Value) {
    let guard = state.read().await;
    finalize_discovery_current(&guard, agent, output, true);
}

pub(crate) fn finalize_discovery_current(
    guard: &crate::state::DaemonState,
    agent: &str,
    output: &mut Value,
    read_authorized: bool,
) {
    let tokens = search_tokens(output["query"].as_str().unwrap_or_default());
    let reading = read_authorized
        && guard
            .agents
            .get(agent)
            .is_some_and(|runtime| runtime.config().allows_tool("read_conversation_window"));
    let Some(results) = output["results"].as_array_mut() else {
        return;
    };
    let original_count = results.len();
    results.retain_mut(|result| {
        let Some(id) = result["sessionId"].as_str() else {
            return false;
        };
        let Some(record) = guard
            .sessions
            .get(agent, id)
            .filter(|record| allowed(guard, agent, record))
        else {
            return false;
        };
        let hot = guard.agents[agent]
            .messages()
            .iter()
            .filter(|message| message.room_id == record.room_id())
            .collect::<Vec<_>>();
        let hidden = hidden_message_ids(hot.iter().copied());
        if let Some(anchor) = result["matchMessageId"].as_str() {
            if hidden.contains(anchor) {
                return false;
            }
            if let Some(message) = hot.iter().find(|message| message.id == anchor) {
                if message.agent_id != agent
                    || message.role == MessageRole::System
                    || !valid_id(&message.id)
                    || result["matchedAtMs"].as_u64() != Some(message.created_at_ms)
                    || (result["matchKind"] == "message"
                        && !text_matches(searchable_text(message), &tokens))
                {
                    return false;
                }
                let snippet = search_snippet(snippet_text(message), &tokens);
                result["snippetShortened"] = json!(snippet != display_text(message));
                result["snippet"] = json!(snippet);
                result["role"] = json!(role_name(message.role));
            }
        }
        result["title"] = json!(clip(&record.title, 240));
        if !reading {
            result
                .as_object_mut()
                .expect("result object")
                .remove("context");
        } else if let Some(context) = result.get_mut("context") {
            if let Some(rows) = context["messages"].as_array_mut() {
                let old_len = rows.len();
                rows.retain_mut(|row| {
                    let Some(id) = row["messageId"].as_str() else {
                        return false;
                    };
                    if hidden.contains(id) {
                        return false;
                    }
                    if let Some(message) = hot.iter().find(|message| message.id == id) {
                        if message.agent_id != agent
                            || message.role == MessageRole::System
                            || row["createdAtMs"].as_u64() != Some(message.created_at_ms)
                        {
                            return false;
                        }
                        *row = shape_message(message);
                    }
                    true
                });
                if rows.len() != old_len {
                    context["shortened"] = json!(true);
                    context["hasEarlier"] = json!(true);
                    context["hasLater"] = json!(true);
                    context["sourceCoverage"] = json!("bounded_with_omissions");
                }
                let anchor = context["anchorMessageId"].as_str().unwrap_or_default();
                if !context["messages"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|row| row["messageId"] == anchor))
                {
                    *context = missing(&record.id, anchor);
                } else {
                    context["pairingIncomplete"] = json!(
                        context["pairingIncomplete"] == true
                            || pairing_incomplete(
                                context["messages"].as_array().expect("context messages")
                            )
                    );
                    bound_window_to(context, super::WINDOW_BYTES);
                }
            }
        }
        true
    });
    let removed = results.len() != original_count;
    let has_shortened = results
        .iter()
        .any(|result| result["snippetShortened"] == true || result["titleShortened"] == true);
    output["status"] = json!(if results.is_empty() {
        "not_found"
    } else {
        "found"
    });
    if removed || has_shortened {
        output["shortened"] = json!(true);
    }
    let context = output["results"]
        .as_array_mut()
        .and_then(|results| results.first_mut())
        .and_then(Value::as_object_mut)
        .and_then(|first| first.remove("context"));
    while size(output) > DISCOVERY_BYTES {
        if output["results"]
            .as_array_mut()
            .expect("discovery results")
            .pop()
            .is_none()
        {
            break;
        }
        output["shortened"] = json!(true);
        output["hasMore"] = json!(true);
        if output["mode"] == "browse" {
            if let Some(last) = output["results"].as_array().and_then(|rows| rows.last()) {
                output["nextCursor"] = json!(format!(
                    "{}:{}",
                    last["lastActivityAtMs"].as_u64().expect("activity time"),
                    last["sessionId"].as_str().expect("session ID")
                ));
            }
        }
    }
    if let Some(mut context) = context {
        let budget = DISCOVERY_BYTES.saturating_sub(size(output) + 32);
        if budget >= 1024 {
            bound_window_to(&mut context, budget);
            if size(&context) <= budget {
                output["results"][0]["context"] = context;
            }
        }
    }
}

async fn newest_message(
    state: &SharedDaemonState,
    agent: &str,
    session_id: &str,
) -> Result<Option<Message>, HistoryReadError> {
    let (record, hot, hidden, store) = {
        let guard = state.read().await;
        let Some(record) = guard
            .sessions
            .get(agent, session_id)
            .filter(|record| allowed(&guard, agent, record))
        else {
            return Ok(None);
        };
        let hot = guard.agents[agent]
            .messages()
            .iter()
            .filter(|message| message.room_id == record.room_id())
            .cloned()
            .collect::<Vec<_>>();
        if hot.len() > 5000 {
            return Err(HistoryReadError::ScopeLimit);
        }
        let hidden = hidden_message_ids(hot.iter());
        (record.clone(), hot, hidden, guard.history.store())
    };
    let hot_ids: HashSet<_> = hot.iter().map(|message| message.id.as_str()).collect();
    let mut latest = hot
        .iter()
        .filter(|message| {
            message.agent_id == agent
                && message.role != MessageRole::System
                && !hidden.contains(&message.id)
                && valid_id(&message.id)
        })
        .max_by_key(|message| MessageOrder::of(message))
        .cloned();
    let mut before = None;
    for _ in 0..5 {
        let rows = store
            .page_messages(&MessagePageQuery {
                agent_id: agent.into(),
                session_id: session_id.into(),
                before: before.clone(),
                limit: 20,
                include_hidden: false,
            })
            .await
            .map_err(|_| HistoryReadError::Unavailable)?;
        if rows.len() > 20 {
            return Err(HistoryReadError::Unavailable);
        }
        let exhausted = rows.len() < 20;
        for row in &rows {
            if row.agent_id != agent
                || row.session_id != record.id
                || row.message.agent_id != agent
                || row.message.room_id != record.room_id()
                || !valid_id(&row.message.id)
                || before
                    .as_ref()
                    .is_some_and(|boundary| row.order() >= *boundary)
            {
                return Err(HistoryReadError::Unavailable);
            }
            if !hot_ids.contains(row.message.id.as_str())
                && row_allowed(row, agent, &record, &hidden)
                && latest
                    .as_ref()
                    .is_none_or(|message| MessageOrder::of(message) < row.order())
            {
                latest = Some(row.message.clone());
            }
        }
        if exhausted
            || rows.last().is_some_and(|last| {
                latest
                    .as_ref()
                    .is_some_and(|message| MessageOrder::of(message) >= last.order())
            })
        {
            return Ok(latest);
        }
        before = rows.last().map(|row| row.order());
    }
    Err(HistoryReadError::Unavailable)
}

pub(crate) async fn read_latest_conversation(
    state: &SharedDaemonState,
    agent: &str,
    session_id: &str,
    before_count: usize,
) -> Result<Value, HistoryReadError> {
    if !valid_id(session_id) || before_count > 19 {
        return Err(HistoryReadError::InvalidRequest);
    }
    tokio::time::timeout(DEADLINE, async {
        let Some(anchor)=newest_message(state,agent,session_id).await? else {
            return Ok(missing(session_id,""));
        };
        let mut output=read_conversation_window(state,agent,WindowRequest {
            session_id:session_id.into(),anchor_message_id:anchor.id,before_count,after_count:0,
        }).await?;
        output["mode"]=json!("recent");
        output["hint"]=json!("Read earlier with anchor_message_id=firstMessageId and before_count=19, after_count=0. Read later with anchor_message_id=lastMessageId and before_count=0, after_count=19. Boundary messages repeat.");
        bound_window_to(&mut output,super::WINDOW_BYTES);
        Ok(output)
    }).await.map_err(|_| HistoryReadError::Unavailable)?
}

pub(crate) async fn hydrate_top_result(
    state: &SharedDaemonState,
    agent: &str,
    output: &mut Value,
) -> Result<(), HistoryReadError> {
    let Some(first) = output["results"].as_array().and_then(|rows| rows.first()) else {
        return Ok(());
    };
    let Some(session_id) = first["sessionId"].as_str().map(str::to_owned) else {
        return Ok(());
    };
    let anchor = first["matchMessageId"].as_str().map(str::to_owned);
    let mut context = if let Some(anchor_message_id) = anchor {
        read_conversation_window(
            state,
            agent,
            WindowRequest {
                session_id,
                anchor_message_id,
                before_count: 3,
                after_count: 3,
            },
        )
        .await?
    } else {
        read_latest_conversation(state, agent, &session_id, 6).await?
    };
    let budget = DISCOVERY_BYTES.saturating_sub(size(output) + 32);
    if budget < 1024 {
        return Ok(());
    }
    bound_window_to(&mut context, budget);
    if size(&context) <= budget {
        output["results"][0]["context"] = context;
    }
    Ok(())
}
