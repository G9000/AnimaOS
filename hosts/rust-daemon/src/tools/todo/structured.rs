//! ID-aware access to the same canonical per-agent task store.
use super::*;
use anima_core::structured_tasks::{self as tasks, ParentChange, TaskPatch, TaskRecord, TaskState};

pub(super) fn records_from_items(
    items: &[TodoItem],
    revision: &str,
) -> Result<Vec<TaskRecord>, String> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Ok(TaskRecord {
                id: item
                    .id
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(|| tasks::provisional_id(revision, index))
                    .map_err(|e| format!("Task identity unavailable: {e:?}"))?,
                content: item.content.clone(),
                active_form: item.active_form.clone(),
                parent: item.parent.clone(),
                status: serde_json::from_value(serde_json::Value::String(item.status.clone()))
                    .map_err(|_| "Invalid task status".to_string())?,
            })
        })
        .collect()
}

fn reply(snapshot: &AgentTodos) -> Result<(Content, bool), String> {
    let records = records_from_items(&snapshot.tasks, &snapshot.revision)?;
    tasks::validate(&records).map_err(|e| format!("Invalid structured tasks: {e:?}"))?;
    let count = |status| records.iter().filter(|t| t.status == status).count();
    let unfinished = records.iter().filter(|t| t.status.is_active()).count();
    let mut value = serde_json::json!({"todos":records,"revision":snapshot.revision,"objectsOmitted":false,
        "summary":{"pending":count(TaskState::Pending),"inProgress":count(TaskState::InProgress),
            "completed":count(TaskState::Completed),"cancelled":count(TaskState::Cancelled),"unfinished":unfinished}});
    let mut text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    let complete = text.len() <= 32 * 1024;
    if !complete {
        value["todos"] = serde_json::Value::Null;
        value["objectsOmitted"] = serde_json::Value::Bool(true);
        value["limitation"] = serde_json::Value::String("Full saved task objects exceed the reply limit. Nothing was truncated or removed. Use the full workspace task view before replacement.".into());
        text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    }
    Ok((
        Content {
            text,
            attachments: None,
            metadata: Some(
                [(
                    "structuredTodoUnfinished".into(),
                    DataValue::Number(unfinished as f64),
                )]
                .into(),
            ),
        },
        complete,
    ))
}

fn patch(value: &DataValue) -> Result<TaskPatch, String> {
    let DataValue::Object(fields) = value else {
        return Err("Every task must be an object".into());
    };
    if fields.keys().any(|k| {
        !matches!(
            k.as_str(),
            "id" | "content" | "status" | "activeForm" | "parent"
        )
    }) {
        return Err("Unknown structured task field".into());
    }
    let string = |key| match fields.get(key) {
        None => Ok(None),
        Some(DataValue::String(value)) => Ok(Some(value.clone())),
        _ => Err(format!("Task {key} must be a string")),
    };
    Ok(TaskPatch {
        id: string("id")?.ok_or("Every structured task requires an explicit id")?,
        content: string("content")?,
        active_form: string("activeForm")?,
        status: string("status")?
            .map(|value| {
                serde_json::from_value(serde_json::Value::String(value)).map_err(|_| {
                    "Task status must be pending | in_progress | completed | cancelled".to_string()
                })
            })
            .transpose()?,
        parent: match fields.get("parent") {
            None => ParentChange::Keep,
            Some(DataValue::Null) => ParentChange::Set(None),
            Some(DataValue::String(parent)) => ParentChange::Set(Some(parent.clone())),
            _ => return Err("Task parent must be a string or null".into()),
        },
    })
}

pub(super) fn register(registry: &mut super::super::ToolRegistry) {
    use serde_json::json;
    fn data(value: serde_json::Value) -> DataValue {
        match value {
            serde_json::Value::Null => DataValue::Null,
            serde_json::Value::Bool(v) => DataValue::Bool(v),
            serde_json::Value::String(v) => DataValue::String(v),
            serde_json::Value::Number(v) => {
                DataValue::Number(v.as_f64().expect("static bounded number"))
            }
            serde_json::Value::Array(v) => DataValue::Array(v.into_iter().map(data).collect()),
            serde_json::Value::Object(v) => {
                DataValue::Object(v.into_iter().map(|(k, v)| (k, data(v))).collect())
            }
        }
    }
    let DataValue::Object(schema) = data(
        json!({"type":"object","additionalProperties":false,"required":[],"properties":{
        "merge":{"type":"boolean","description":"Update supplied fields by exact ID, retaining all omitted records. Default false replaces the complete list."},
        "todos":{"type":"array","maxItems":256,"items":{"type":"object","additionalProperties":false,"required":["id"],"properties":{
            "id":{"type":"string","minLength":1,"maxLength":128},"parent":{"type":["string","null"],"minLength":1,"maxLength":128},
            "content":{"type":"string","minLength":1,"maxLength":4000},"activeForm":{"type":"string","minLength":1,"maxLength":4000},
            "status":{"type":"string","enum":["pending","in_progress","completed","cancelled"]}
        }}}}}),
    ) else {
        unreachable!()
    };
    registry.register(super::super::tool_descriptor("todo_list", "Read saved tasks with stable IDs, hierarchy and cancellation. Omit todos to read without changing disk. To write, first read the current complete list; merge:true changes only supplied fields by exact ID and appends fully specified new records. parent:null explicitly clears a relation. Stale revisions save nothing and require a deliberate retry against the returned current list. Never guess identities from descriptions.", schema), execute_todo_list);
}

pub(super) fn execute_todo_list(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let operation = || -> Result<Content, String> {
            if call
                .args
                .keys()
                .any(|k| !matches!(k.as_str(), "todos" | "merge"))
            {
                return Err("Unknown todo_list argument".into());
            }
            let merge = match call.args.get("merge") {
                None => false,
                Some(DataValue::Bool(v)) => *v,
                _ => return Err("merge must be a boolean".into()),
            };
            let root = ctx_workspace_root(&context);
            let owner = context.todo_owner_id(&agent);
            let current = read_agent_todos(root, owner)?;
            let Some(input) = call.args.get("todos") else {
                if merge {
                    return Err("merge requires todos".into());
                }
                let (content, complete) = reply(&current)?;
                *context
                    .todo_revision
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = complete.then_some(current.revision);
                return Ok(content);
            };
            let DataValue::Array(input) = input else {
                return Err("todos must be an array".into());
            };
            if input.len() > tasks::MAX_TASKS {
                return Err("Too many structured tasks".into());
            }
            let observed = context.todo_revision.lock().unwrap_or_else(|p| p.into_inner()).clone()
                .ok_or("No complete task revision observed. Call todo_list without todos before writing.")?;
            if observed != current.revision {
                let (content, complete) = reply(&current)?;
                *context
                    .todo_revision
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = complete.then_some(current.revision);
                return Err(format!("Tasks changed. Nothing was saved. Reconcile with this current saved list and deliberately retry:\n{}", content.text));
            }
            let patches = input.iter().map(patch).collect::<Result<Vec<_>, _>>()?;
            let records = if merge {
                records_from_items(&current.tasks, &current.revision)?
            } else {
                Vec::new()
            };
            let proposed = tasks::merge(&records, &patches)
                .map_err(|e| format!("Invalid structured tasks: {e:?}"))?;
            let items = proposed
                .into_iter()
                .map(|task| TodoItem {
                    parent_specified: patches.iter().any(|patch| {
                        patch.id == task.id && matches!(patch.parent, ParentChange::Set(_))
                    }),
                    id: Some(task.id),
                    parent: task.parent,
                    content: task.content,
                    active_form: task.active_form,
                    status: match task.status {
                        TaskState::Pending => "pending",
                        TaskState::InProgress => "in_progress",
                        TaskState::Completed => "completed",
                        TaskState::Cancelled => "cancelled",
                    }
                    .into(),
                })
                .collect::<Vec<_>>();
            if serde_json::to_vec_pretty(&items)
                .map_err(|e| e.to_string())?
                .len()
                > tasks::MAX_TASK_PAYLOAD_BYTES
            {
                return Err("Serialized tasks exceed the saved payload limit".into());
            }
            match write_agent_todos(root, owner, &items, Some(&observed)) {
                Ok(saved) => {
                    let (content, complete) = reply(&saved)?;
                    *context
                        .todo_revision
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = complete.then_some(saved.revision);
                    Ok(content)
                }
                Err(error) if error.starts_with(TASKS_CHANGED_PREFIX) => {
                    let latest = read_agent_todos(root, owner)?;
                    let (content, complete) = reply(&latest)?;
                    *context
                        .todo_revision
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = complete.then_some(latest.revision);
                    Err(format!("Tasks changed. Nothing was saved. Reconcile with this current saved list and deliberately retry:\n{}", content.text))
                }
                Err(error) => Err(error),
            }
        };
        match operation() {
            Ok(content) => TaskResult::success(content, 0),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Arc;

    fn value(json: Value) -> DataValue {
        match json {
            Value::Null => DataValue::Null,
            Value::Bool(v) => DataValue::Bool(v),
            Value::Number(v) => DataValue::Number(v.as_f64().unwrap()),
            Value::String(v) => DataValue::String(v),
            Value::Array(v) => DataValue::Array(v.into_iter().map(value).collect()),
            Value::Object(v) => {
                DataValue::Object(v.into_iter().map(|(k, v)| (k, value(v))).collect())
            }
        }
    }
    fn call(args: Value) -> ToolCall {
        let DataValue::Object(args) = value(args) else {
            panic!("object fixture");
        };
        ToolCall {
            id: "structured-call".into(),
            name: "todo_list".into(),
            args,
        }
    }
    fn agent() -> AgentState {
        AgentState {
            id: "structured-agent".into(),
            name: "Task agent".into(),
            status: anima_core::AgentStatus::Idle,
            config: crate::agent_runs::test_support::companion_config("Task agent"),
            created_at_ms: 1,
            token_usage: Default::default(),
        }
    }
    fn message() -> Message {
        Message {
            id: "task-input".into(),
            agent_id: agent().id,
            room_id: "chat:tasks".into(),
            content: Content::default(),
            role: anima_core::MessageRole::User,
            created_at_ms: 1,
        }
    }
    fn context(root: &Path) -> ToolExecutionContext {
        ToolExecutionContext::new(
            Arc::new(tokio::sync::RwLock::new(anima_memory::MemoryManager::new())),
            Arc::new(tokio::sync::RwLock::new(
                crate::memory_embeddings::MemoryEmbeddingRuntime::disabled(),
            )),
            None,
            crate::tools::ToolRegistry::new(),
            crate::tools::new_shared_process_manager_with_limit(1),
            Some(root.to_path_buf()),
            None,
        )
    }
    async fn invoke(context: &ToolExecutionContext, args: Value) -> TaskResult<Content> {
        execute_todo_list(context.clone(), agent(), message(), call(args)).await
    }
    fn reply(result: TaskResult<Content>) -> Value {
        assert_eq!(
            result.status,
            anima_core::TaskStatus::Success,
            "{:?}",
            result.error
        );
        serde_json::from_str(&result.data.unwrap().text).unwrap()
    }
    fn full_tasks() -> Value {
        json!([
            {"id":"root","content":"Preserve exact root","status":"completed","activeForm":"Completing root"},
            {"id":"child","parent":"root","content":"Preserve exact child","status":"in_progress","activeForm":"Working on child"}
        ])
    }

    async fn oversized_legacy_reply(kind: &str) {
        let private = tempfile::tempdir().unwrap();
        let context = context(private.path());
        let large = Value::Array((0..10).map(|i| json!({"id":format!("t{i}"),"content":"x".repeat(3900),"activeForm":"Working","status":"pending"})).collect());
        let items: Vec<TodoItem> = serde_json::from_value(large.clone()).unwrap();
        let initial: Vec<TodoItem> = if kind == "conflict" {
            serde_json::from_value(full_tasks()).unwrap()
        } else {
            items.clone()
        };
        let initial = write_agent_todos(Some(private.path()), &agent().id, &initial, None).unwrap();
        *context.todo_revision.lock().unwrap() = Some(initial.revision);
        if kind == "conflict" {
            write_agent_todos(Some(private.path()), &agent().id, &items, None).unwrap();
        }
        let mut request = call(if kind == "conflict" {
            json!({"todos":full_tasks()})
        } else {
            json!({"todos":large})
        });
        request.name = "todo_write".into();
        let result = if kind == "read" {
            execute_todo_read(context.clone(), agent(), message(), request).await
        } else {
            execute_todo_write(context.clone(), agent(), message(), request).await
        };
        if kind == "conflict" {
            assert_eq!(result.status, anima_core::TaskStatus::Error);
        } else {
            assert_eq!(
                result.status,
                anima_core::TaskStatus::Success,
                "{:?}",
                result.error
            );
        }
        assert!(
            context.todo_revision.lock().unwrap().is_none(),
            "{kind}: omitted complete objects cannot establish replacement authority"
        );
    }
    #[tokio::test]
    async fn fp03_review_oversized_legacy_read_clears_baseline() {
        oversized_legacy_reply("read").await;
    }
    #[tokio::test]
    async fn fp03_review_oversized_legacy_write_clears_baseline() {
        oversized_legacy_reply("write").await;
    }
    #[tokio::test]
    async fn fp03_review_oversized_legacy_conflict_clears_baseline() {
        oversized_legacy_reply("conflict").await;
    }

    #[tokio::test]
    async fn fp03_review_legacy_missing_parent_refuses_but_explicit_null_clears() {
        let private = tempfile::tempdir().unwrap();
        let context = context(private.path());
        reply(invoke(&context, json!({})).await);
        reply(invoke(&context, json!({"todos":full_tasks()})).await);
        let path = agent_todo_path(Some(private.path()), &agent().id).unwrap();
        let bytes = fs::read(&path).unwrap();
        let mut tasks = full_tasks();
        tasks[1].as_object_mut().unwrap().remove("parent");
        let mut request = call(json!({"todos":tasks}));
        request.name = "todo_write".into();
        let result = execute_todo_write(context.clone(), agent(), message(), request).await;
        assert_eq!(
            result.status,
            anima_core::TaskStatus::Error,
            "omitted parent cannot silently flatten saved hierarchy"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        tasks[1]["parent"] = Value::Null;
        let mut request = call(json!({"todos":tasks}));
        request.name = "todo_write".into();
        assert_eq!(
            execute_todo_write(context, agent(), message(), request)
                .await
                .status,
            anima_core::TaskStatus::Success
        );
        let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(saved[1].get("parent").is_none());
    }

    #[test]
    fn fp03_review_http_task_dto_missing_parent_cannot_flatten_hierarchy() {
        let private = tempfile::tempdir().unwrap();
        let original: Vec<TodoItem> = serde_json::from_value(full_tasks()).unwrap();
        let saved = write_agent_todos(Some(private.path()), &agent().id, &original, None).unwrap();
        let path = agent_todo_path(Some(private.path()), &agent().id).unwrap();
        let bytes = fs::read(&path).unwrap();
        let mut tasks = full_tasks();
        tasks[1].as_object_mut().unwrap().remove("parent");
        let omitted: Vec<TodoItem> = serde_json::from_value(tasks.clone()).unwrap();
        assert!(write_agent_todos(
            Some(private.path()),
            &agent().id,
            &omitted,
            Some(&saved.revision)
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        tasks[1]["parent"] = Value::Null;
        let cleared: Vec<TodoItem> = serde_json::from_value(tasks).unwrap();
        let saved = write_agent_todos(
            Some(private.path()),
            &agent().id,
            &cleared,
            Some(&saved.revision),
        )
        .unwrap();
        assert!(saved.tasks[1].parent.is_none());
    }

    #[tokio::test]
    async fn fp03_review_stale_merge_reports_current_list_before_changed_id_validation() {
        let private = tempfile::tempdir().unwrap();
        let a = context(private.path());
        let b = context(private.path());
        reply(invoke(&a, json!({})).await);
        reply(invoke(&a, json!({"todos":full_tasks()})).await);
        reply(invoke(&b, json!({})).await);
        let latest = reply(invoke(&b, json!({"todos":[full_tasks()[0].clone()]})).await);
        let failed = invoke(
            &a,
            json!({"merge":true,"todos":[{"id":"child","status":"completed"}]}),
        )
        .await;
        assert!(failed.error.unwrap().contains("Tasks changed"));
        assert_eq!(
            a.todo_revision.lock().unwrap().as_deref(),
            latest["revision"].as_str()
        );
    }

    #[tokio::test]
    async fn fp03_structured_host_read_is_lossless_and_assigns_only_provisional_ids() {
        let private = tempfile::tempdir().unwrap();
        let root = private.path();
        let saved = write_agent_todos(
            Some(root),
            &agent().id,
            &[TodoItem {
                parent_specified: false,
                id: None,
                parent: None,
                content: "Legacy description".into(),
                active_form: "Doing legacy work".into(),
                status: "pending".into(),
            }],
            None,
        )
        .unwrap();
        let path = agent_todo_path(Some(root), &agent().id).unwrap();
        let bytes = fs::read(&path).unwrap();
        let context = context(root);
        let first = reply(invoke(&context, json!({})).await);
        let second = reply(invoke(&context, json!({})).await);
        assert_eq!(first, second);
        assert_eq!(first["revision"], saved.revision);
        assert_eq!(first["todos"][0]["content"], "Legacy description");
        assert_eq!(first["todos"][0]["activeForm"], "Doing legacy work");
        assert!(first["todos"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("task_"));
        assert_eq!(
            fs::read(path).unwrap(),
            bytes,
            "read must not migrate the file"
        );
        assert_eq!(
            context.todo_revision.lock().unwrap().as_ref(),
            Some(&saved.revision)
        );
    }

    #[tokio::test]
    async fn fp03_structured_host_merge_persists_identity_parent_and_cancelled_state() {
        let private = tempfile::tempdir().unwrap();
        let context = context(private.path());
        reply(invoke(&context, json!({})).await);
        let first = reply(invoke(&context, json!({"todos":full_tasks()})).await);
        let next = reply(
            invoke(
                &context,
                json!({"merge":true,"todos":[{"id":"child","status":"cancelled"}]}),
            )
            .await,
        );
        assert_ne!(first["revision"], next["revision"]);
        assert_eq!(next["todos"][0], full_tasks()[0]);
        assert_eq!(next["todos"][1]["parent"], "root");
        assert_eq!(next["todos"][1]["content"], "Preserve exact child");
        assert_eq!(next["todos"][1]["activeForm"], "Working on child");
        assert_eq!(next["summary"]["cancelled"], 1);
        assert_eq!(next["summary"]["unfinished"], 0);
        let saved: Value = serde_json::from_slice(
            &fs::read(agent_todo_path(Some(private.path()), &agent().id).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(saved, next["todos"]);
        assert_eq!(reply(invoke(&context, json!({})).await)["todos"], saved);
    }

    #[tokio::test]
    async fn fp03_structured_host_stale_merge_refuses_then_deliberate_retry_preserves_other_room() {
        let private = tempfile::tempdir().unwrap();
        let a = context(private.path());
        let b = context(private.path());
        reply(invoke(&a, json!({})).await);
        reply(invoke(&a, json!({"todos":full_tasks()})).await);
        reply(invoke(&b, json!({})).await);
        let latest = reply(
            invoke(
                &b,
                json!({"merge":true,"todos":[{"id":"root","content":"Owner clarified the root"}]}),
            )
            .await,
        );
        let path = agent_todo_path(Some(private.path()), &agent().id).unwrap();
        let bytes = fs::read(&path).unwrap();
        let conflict = invoke(
            &a,
            json!({"merge":true,"todos":[{"id":"child","status":"completed"}]}),
        )
        .await;
        assert_eq!(conflict.status, anima_core::TaskStatus::Error);
        assert!(conflict.error.unwrap().contains("Tasks changed"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            a.todo_revision.lock().unwrap().as_deref(),
            latest["revision"].as_str()
        );
        let retried = reply(
            invoke(
                &a,
                json!({"merge":true,"todos":[{"id":"child","status":"completed"}]}),
            )
            .await,
        );
        assert_eq!(retried["todos"][0]["content"], "Owner clarified the root");
        assert_eq!(retried["todos"][1]["status"], "completed");
    }

    #[tokio::test]
    async fn fp03_structured_host_invalid_merge_and_legacy_identity_stripping_preserve_bytes() {
        let private = tempfile::tempdir().unwrap();
        let context = context(private.path());
        reply(invoke(&context, json!({})).await);
        reply(invoke(&context, json!({"todos":full_tasks()})).await);
        let path = agent_todo_path(Some(private.path()), &agent().id).unwrap();
        let bytes = fs::read(&path).unwrap();
        for args in [
            json!({"merge":true,"todos":[{"id":"root","parent":"child"}]}),
            json!({"merge":true,"todos":[{"id":"child","invented":true}]}),
            json!({"merge":"yes","todos":[]}),
            json!({"todos":[{"id":"empty"}]}),
        ] {
            assert_eq!(
                invoke(&context, args).await.status,
                anima_core::TaskStatus::Error
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let legacy = ToolCall {
            id: "legacy-write".into(),
            name: "todo_write".into(),
            args: match value(
                json!({"todos":[{"content":"Silent replacement","status":"pending","activeForm":"Doing replacement"}]}),
            ) {
                DataValue::Object(v) => v,
                _ => unreachable!(),
            },
        };
        let refused = execute_todo_write(context, agent(), message(), legacy).await;
        assert_eq!(refused.status, anima_core::TaskStatus::Error);
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
