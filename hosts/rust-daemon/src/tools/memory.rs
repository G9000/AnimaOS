use std::collections::BTreeMap;

use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use anima_memory::{
    Memory, MemoryPatch, MemoryRecallOptions, MemorySearchOptions, MemoryType, NewMemory,
};
use futures::future::BoxFuture;
use tracing::warn;

use crate::memory_store::MemoryMutation;
use crate::memory_text::{
    has_hidden_text, MAX_MEMORY_EDIT_CHARS, MEMORY_CONTENT_INVALID, MEMORY_TEXT_HIDDEN,
};

use super::ToolExecutionContext;

pub(super) fn execute_memory_search(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let query = match tool_call.args.get("query") {
            Some(DataValue::String(value)) if !value.is_empty() => value.clone(),
            _ => {
                return TaskResult::error("memory_search query must be a non-empty string", 0);
            }
        };

        let limit = match tool_call.args.get("limit") {
            Some(DataValue::Number(value))
                if value.is_finite() && *value >= 1.0 && value.fract() == 0.0 =>
            {
                *value as usize
            }
            Some(DataValue::Number(_)) | Some(_) => {
                return TaskResult::error("memory_search limit must be a positive integer", 0);
            }
            None => 3,
        };

        let results = {
            let memory_guard = context.memory.read().await;
            let embeddings_guard = context.memory_embeddings.read().await;
            memory_guard.recall_with_vector_index(
                &query,
                MemoryRecallOptions {
                    search: MemorySearchOptions {
                        agent_id: Some(agent.id.clone()),
                        agent_name: None,
                        memory_type: None,
                        scope: None,
                        room_id: None,
                        world_id: None,
                        session_id: None,
                        limit: Some(limit),
                        min_importance: None,
                    },
                    limit: Some(limit),
                    recent_limit: Some(0),
                    ..MemoryRecallOptions::default()
                },
                Some(&*embeddings_guard),
            )
        }
        .into_iter()
        .filter(|result| !has_hidden_text(&result.memory.content))
        .collect::<Vec<_>>();

        let mut metadata = BTreeMap::new();
        metadata.insert("query".into(), DataValue::String(query));
        metadata.insert("matchCount".into(), DataValue::Number(results.len() as f64));

        let text = if results.is_empty() {
            "no memory matches".to_string()
        } else {
            results
                .into_iter()
                .map(|result| render_memory(&result.memory))
                .collect::<Vec<_>>()
                .join("\n")
        };

        TaskResult::success(
            Content {
                text,
                attachments: None,
                metadata: Some(metadata),
            },
            0,
        )
    })
}

fn render_memory(memory: &Memory) -> String {
    format!(
        "Memory ID: {}\nType: {}\nContent:\n{}",
        memory.id,
        memory.memory_type.as_str(),
        memory.content
    )
}

pub(super) fn execute_memory_update(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let id = match tool_call.args.get("id") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.clone(),
            _ => return TaskResult::error("memory_update id must be a non-empty string", 0),
        };
        let expected = match tool_call.args.get("expected_content") {
            Some(DataValue::String(value)) if !value.is_empty() => value.clone(),
            _ => {
                return TaskResult::error(
                    "memory_update expected_content must be the exact current content",
                    0,
                )
            }
        };
        let content = match tool_call.args.get("content") {
            Some(DataValue::String(value))
                if !value.trim().is_empty()
                    && value.trim().chars().count() <= MAX_MEMORY_EDIT_CHARS =>
            {
                value.trim().to_string()
            }
            _ => return TaskResult::error(MEMORY_CONTENT_INVALID, 0),
        };
        if has_hidden_text(&content) {
            return TaskResult::error(MEMORY_TEXT_HIDDEN, 0);
        }

        let memory = {
            let mut guard = context.memory.write().await;
            // Port of upstream pinned-entry replacement: require the exact record
            // and full reviewed text under the same write lock as persistence.
            let Some(current) = guard.get(&id).filter(|memory| memory.agent_id == agent.id) else {
                return TaskResult::error("memory not found", 0);
            };
            if has_hidden_text(&current.content) {
                return TaskResult::error(MEMORY_TEXT_HIDDEN, 0);
            }
            if current.content != expected {
                return TaskResult::error(
                    "memory changed; search again and use its exact current content",
                    0,
                );
            }
            let mut mutation = MemoryMutation::new(&mut guard);
            let updated = match mutation.update_memory(
                &id,
                MemoryPatch {
                    content: Some(content.clone()),
                    ..MemoryPatch::default()
                },
            ) {
                Ok(Some(memory)) => memory,
                Ok(None) => return TaskResult::error("memory not found", 0),
                Err(error) => return TaskResult::error(error.message(), 0),
            };
            if let Err(error) = mutation.persist(context.memory_store.as_ref()).await {
                return TaskResult::error(format!("failed to persist memory: {error}"), 0);
            }
            updated
        };

        // Re-read canonical content before synchronous indexing; keep the same
        // memory -> embeddings lock order as recall. Vectors remain best effort.
        {
            let guard = context.memory.read().await;
            if let Some(current) = guard.get(&id) {
                if let Err(error) = context
                    .memory_embeddings
                    .write()
                    .await
                    .upsert_memory(&current)
                {
                    warn!(memory_id = %id, error = %error, "failed to index memory_update embedding");
                }
            }
        }
        let metadata = BTreeMap::from([
            ("memoryId".into(), DataValue::String(memory.id)),
            (
                "memoryType".into(),
                DataValue::String(memory.memory_type.as_str().into()),
            ),
        ]);
        TaskResult::success(
            Content {
                text: format!("updated memory: {content}"),
                attachments: None,
                metadata: Some(metadata),
            },
            0,
        )
    })
}

pub(super) fn execute_memory_add(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let content = match tool_call.args.get("content") {
            Some(DataValue::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            _ => return TaskResult::error("memory_add content must be a non-empty string", 0),
        };
        if has_hidden_text(&content) {
            return TaskResult::error(MEMORY_TEXT_HIDDEN, 0);
        }

        let memory_type =
            match tool_call.args.get("type") {
                None => MemoryType::Fact,
                Some(DataValue::String(value)) => match MemoryType::parse(value) {
                    Ok(memory_type) => memory_type,
                    Err(()) => return TaskResult::error(
                        "memory_add type must be one of fact, observation, task_result, reflection",
                        0,
                    ),
                },
                Some(_) => return TaskResult::error("memory_add type must be a string", 0),
            };

        let importance = match tool_call.args.get("importance") {
            None => 0.8,
            Some(DataValue::Number(value)) if value.is_finite() && (0.0..=1.0).contains(value) => {
                *value
            }
            Some(DataValue::Number(_)) | Some(_) => {
                return TaskResult::error("memory_add importance must be between 0 and 1", 0);
            }
        };

        let memory = {
            let mut memory_guard = context.memory.write().await;
            let mut memory_guard = MemoryMutation::new(&mut memory_guard);
            let memory = match memory_guard.add(NewMemory {
                agent_id: agent.id.clone(),
                agent_name: agent.name.clone(),
                memory_type,
                content: content.clone(),
                importance,
                tags: Some(vec!["runtime".into(), "tool-memory-add".into()]),
                scope: None,
                room_id: None,
                world_id: None,
                session_id: None,
            }) {
                Ok(memory) => memory,
                Err(error) => return TaskResult::error(error.message(), 0),
            };
            if let Err(error) = memory_guard.persist(context.memory_store.as_ref()).await {
                return TaskResult::error(format!("failed to persist memory: {error}"), 0);
            }
            memory
        };

        if let Err(error) = context
            .memory_embeddings
            .write()
            .await
            .upsert_memory(&memory)
        {
            warn!(
                memory_id = %memory.id,
                error = %error,
                "failed to index memory_add embedding"
            );
        }

        let mut metadata = BTreeMap::new();
        metadata.insert("memoryId".into(), DataValue::String(memory.id));
        metadata.insert(
            "memoryType".into(),
            DataValue::String(memory.memory_type.as_str().to_string()),
        );

        TaskResult::success(
            Content {
                text: format!("stored memory: {content}"),
                attachments: None,
                metadata: Some(metadata),
            },
            0,
        )
    })
}

pub(super) fn execute_recent_memories(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let limit = match tool_call.args.get("limit") {
            Some(DataValue::Number(value))
                if value.is_finite() && *value >= 1.0 && value.fract() == 0.0 =>
            {
                *value as usize
            }
            Some(DataValue::Number(_)) | Some(_) => {
                return TaskResult::error("recent_memories limit must be a positive integer", 0);
            }
            None => 3,
        };

        let memories = context
            .memory
            .read()
            .await
            .get_recent(anima_memory::RecentMemoryOptions {
                agent_id: Some(agent.id.clone()),
                agent_name: None,
                scope: None,
                room_id: None,
                world_id: None,
                session_id: None,
                limit: Some(limit),
            })
            .into_iter()
            .filter(|memory| !has_hidden_text(&memory.content))
            .collect::<Vec<_>>();

        let mut metadata = BTreeMap::new();
        metadata.insert("limit".into(), DataValue::Number(limit as f64));
        metadata.insert(
            "matchCount".into(),
            DataValue::Number(memories.len() as f64),
        );

        let text = if memories.is_empty() {
            "no recent memories".to_string()
        } else {
            memories
                .into_iter()
                .map(|memory| render_memory(&memory))
                .collect::<Vec<_>>()
                .join("\n")
        };

        TaskResult::success(
            Content {
                text,
                attachments: None,
                metadata: Some(metadata),
            },
            0,
        )
    })
}
