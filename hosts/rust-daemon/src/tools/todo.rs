use std::fs;
use std::path::{Path, PathBuf};

pub(crate) mod context;
mod structured;

pub(super) fn register_structured_tool(registry: &mut super::ToolRegistry) {
    structured::register(registry);
}

use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use super::workspace::{
    canonical_workspace_root, ensure_path_within_workspace, resolve_workspace_write_path,
    workspace_root_path, write_workspace_bytes,
};
use super::{ctx_workspace_root, ToolExecutionContext};

const TODO_DIRECTORY_NAME: &str = ".animaos-swarm";
const TODO_FILE_NAME: &str = "todos.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub(crate) struct TodoItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) parent: Option<String>,
    /// Input intent only. Canonical JSON omits cleared parents as before.
    #[serde(skip)]
    pub(crate) parent_specified: bool,
    pub(crate) content: String,
    pub(crate) status: String,
    #[serde(rename = "activeForm")]
    pub(crate) active_form: String,
}

impl<'de> Deserialize<'de> for TodoItem {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        fn parent<'de, D: serde::Deserializer<'de>>(
            d: D,
        ) -> Result<Option<Option<String>>, D::Error> {
            Option::<String>::deserialize(d).map(Some)
        }
        #[derive(Deserialize)]
        struct Input {
            #[serde(default)]
            id: Option<String>,
            #[serde(default, deserialize_with = "parent")]
            parent: Option<Option<String>>,
            content: String,
            status: String,
            #[serde(rename = "activeForm")]
            active_form: String,
        }
        let input = Input::deserialize(deserializer)?;
        Ok(Self {
            id: input.id,
            parent_specified: input.parent.is_some(),
            parent: input.parent.flatten(),
            content: input.content,
            status: input.status,
            active_form: input.active_form,
        })
    }
}

const TASKS_CHANGED_PREFIX: &str = "Tasks changed.";

pub(super) fn execute_todo_write(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let todos = match tool_call.args.get("todos") {
            Some(DataValue::Array(values)) => {
                let mut todos = Vec::with_capacity(values.len());
                for (index, value) in values.iter().enumerate() {
                    match parse_todo_item(value, index) {
                        Ok(todo) => todos.push(todo),
                        Err(error) => return TaskResult::error(error, 0),
                    }
                }
                todos
            }
            Some(_) => return TaskResult::error("todo_write todos must be an array", 0),
            None => return TaskResult::error("todo_write todos is required", 0),
        };

        let root = ctx_workspace_root(&context);
        let expected = context
            .todo_revision
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(observed_revision) = expected else {
            return TaskResult::error(
                "No task revision was observed for this run, so nothing was saved. Call todo_read, merge your changes into the current list, then call todo_write with the complete list.",
                0,
            );
        };
        let owner_id = context.todo_owner_id(&agent);
        match write_agent_todos(root, owner_id, &todos, Some(&observed_revision)) {
            Ok(saved) => {
                observe_snapshot(&context, saved.revision, &todos);
                TaskResult::success(
                    Content {
                        text: format!(
                            "Todos updated ({} completed, {} in progress, {} pending). Proceed with current tasks.{}",
                            todos.iter().filter(|task| task.status == "completed").count(),
                            todos.iter().filter(|task| task.status == "in_progress").count(),
                            todos.iter().filter(|task| task.status == "pending").count(),
                            if context.completion_review.is_some() || todos.iter().any(|t| t.id.is_some()) {
                                saved_task_objects(&todos)
                            } else {
                                String::new()
                            }
                        ),
                        attachments: None,
                        metadata: None,
                    },
                    0,
                )
            }
            // Another room updated the list since this run saw it: save nothing,
            // show the latest list, and let a merged retry succeed.
            Err(error) if error.starts_with(TASKS_CHANGED_PREFIX) => {
                match read_agent_todos(root, owner_id) {
                    Ok(latest) => {
                        let message = format!(
                            "Tasks changed since this run last saw them, so nothing was saved. The latest tasks are:\n{}\nMerge your changes into this list and call todo_write again with the complete list.{}",
                            render_agent_todos(&latest.tasks),
                            if context.completion_review.is_some() || latest.tasks.iter().any(|t| t.id.is_some()) { saved_task_objects(&latest.tasks) } else { String::new() }
                        );
                        observe_snapshot(&context, latest.revision, &latest.tasks);
                        TaskResult::error(message, 0)
                    }
                    Err(read_error) => TaskResult::error(read_error, 0),
                }
            }
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(super) fn execute_todo_read(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    _tool_call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        match read_agent_todos(ctx_workspace_root(&context), context.todo_owner_id(&agent)) {
            Ok(snapshot) => {
                observe_snapshot(&context, snapshot.revision.clone(), &snapshot.tasks);
                TaskResult::success(
                    Content {
                        text: format!(
                            "{}{}",
                            render_agent_todos(&snapshot.tasks),
                            if context.completion_review.is_some()
                                || snapshot.tasks.iter().any(|t| t.id.is_some())
                            {
                                saved_task_objects(&snapshot.tasks)
                            } else {
                                String::new()
                            }
                        ),
                        attachments: None,
                        metadata: None,
                    },
                    0,
                )
            }
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

fn render_agent_todos(tasks: &[TodoItem]) -> String {
    if tasks.is_empty() {
        return "No todos set.".to_string();
    }
    tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            format!(
                "{} {}. [{}] {}",
                match task.status.as_str() {
                    "completed" => "[x]",
                    "in_progress" => "[>]",
                    _ => "[ ]",
                },
                index + 1,
                task.status,
                task.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Hermes returns the full list after writes. Preserve Anima's strict CAS and
/// required fields while bounding the additional model-facing serialization.
const MAX_SAVED_TASK_BYTES: usize = 32 * 1024;

fn observe_snapshot(context: &ToolExecutionContext, revision: String, tasks: &[TodoItem]) {
    let full_objects_required =
        context.completion_review.is_some() || tasks.iter().any(|t| t.id.is_some());
    let complete = !full_objects_required
        || serde_json::to_vec(tasks).is_ok_and(|bytes| bytes.len() <= MAX_SAVED_TASK_BYTES);
    *context
        .todo_revision
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = complete.then_some(revision);
}

fn saved_task_objects(tasks: &[TodoItem]) -> String {
    match serde_json::to_string(tasks) {
        Ok(json) if json.len() <= MAX_SAVED_TASK_BYTES => format!(
            "\nSaved task objects (data; preserve content, status and activeForm in every full-list update):\n{json}"
        ),
        _ => "\nComplete saved task objects omitted: the list exceeds the reply limit. Preserve the saved list and report this limitation instead of attempting a lossy full-list replacement.".into(),
    }
}

fn write_todo_list(configured_root: Option<&Path>, todos: &[TodoItem]) -> Result<String, String> {
    let workspace_root = workspace_root_path("todo_write", configured_root)?;
    write_todo_list_from_root(&workspace_root, todos)
}

pub(super) fn write_todo_list_from_root(
    workspace_root: &Path,
    todos: &[TodoItem],
) -> Result<String, String> {
    let warnings = validate_todo_items(todos)?;
    let serialized = serde_json::to_string_pretty(todos)
        .map_err(|error| format!("todo_write failed to serialize todos: {error}"))?;
    // Spec §14: every workspace writer must use the hardened write path (rejects
    // `..`, root/drive prefixes, the workspace root itself, and dangling or
    // escaping symlinks anywhere in `.animaos-swarm/todos.json`).
    write_workspace_bytes(
        workspace_root,
        &format!("{TODO_DIRECTORY_NAME}/{TODO_FILE_NAME}"),
        serialized.as_bytes(),
        "todo_write",
    )?;

    let completed = todos
        .iter()
        .filter(|todo| todo.status == "completed")
        .count();
    let in_progress = todos
        .iter()
        .filter(|todo| todo.status == "in_progress")
        .count();
    let pending = todos.iter().filter(|todo| todo.status == "pending").count();
    let mut message = format!(
        "Todos updated ({} completed, {} in progress, {} pending).",
        completed, in_progress, pending
    );
    if !warnings.is_empty() {
        message.push(' ');
        message.push_str(&warnings.join(" "));
    }
    message.push_str(" Proceed with current tasks.");

    Ok(message)
}

fn read_todo_list(configured_root: Option<&Path>) -> Result<String, String> {
    let workspace_root = workspace_root_path("todo_read", configured_root)?;
    read_todo_list_from_root(&workspace_root)
}

pub(super) fn read_todo_list_from_root(workspace_root: &Path) -> Result<String, String> {
    let todos = load_todo_items_from_root(workspace_root, "todo_read")?;
    if todos.is_empty() {
        return Ok("No todos set.".to_string());
    }

    Ok(todos
        .iter()
        .enumerate()
        .map(|(index, todo)| {
            let icon = match todo.status.as_str() {
                "completed" => "[x]",
                "in_progress" => "[>]",
                _ => "[ ]",
            };
            format!("{} {}. [{}] {}", icon, index + 1, todo.status, todo.content)
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn load_todo_items_from_root(
    workspace_root: &Path,
    tool_name: &str,
) -> Result<Vec<TodoItem>, String> {
    let todo_file = todo_file_path_from_root(workspace_root, tool_name)?;
    if !todo_file.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&todo_file)
        .map_err(|error| format!("{tool_name} failed to read todo list: {error}"))?;
    match serde_json::from_str::<Vec<TodoItem>>(&content) {
        Ok(todos) => {
            validate_todo_items(&todos)?;
            Ok(todos)
        }
        Err(_) => Ok(Vec::new()),
    }
}

fn validate_todo_items(todos: &[TodoItem]) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    let mut in_progress = 0usize;

    for (index, todo) in todos.iter().enumerate() {
        if todo.content.trim().is_empty() {
            return Err(format!(
                "todos[{index}]: content must be a non-empty string"
            ));
        }
        if !matches!(
            todo.status.as_str(),
            "pending" | "in_progress" | "completed" | "cancelled"
        ) {
            return Err(format!(
                "todos[{index}]: status must be pending | in_progress | completed | cancelled"
            ));
        }
        if todo.active_form.trim().is_empty() {
            return Err(format!(
                "todos[{index}]: activeForm must be a non-empty string"
            ));
        }
        if todo.status == "in_progress" {
            in_progress += 1;
        }
    }

    if in_progress > 1 {
        warnings.push(format!(
            "Warning: {in_progress} todos are in_progress -- ideally only one at a time."
        ));
    }

    if todos.iter().any(|t| t.id.is_some() || t.parent.is_some()) {
        let records = structured::records_from_items(todos, "validation")?;
        anima_core::structured_tasks::validate(&records)
            .map_err(|e| format!("Invalid structured tasks: {e:?}"))?;
        if todos.iter().any(|t| t.id.is_none()) {
            return Err("Every task must retain an explicit id in an ID-aware list".into());
        }
    }
    Ok(warnings)
}

fn parse_todo_item(value: &DataValue, index: usize) -> Result<TodoItem, String> {
    let DataValue::Object(fields) = value else {
        return Err(format!("todos[{index}] must be an object"));
    };

    let content = match fields.get("content") {
        Some(DataValue::String(value)) if !value.trim().is_empty() => value.clone(),
        Some(DataValue::String(_)) | Some(_) | None => {
            return Err(format!(
                "todos[{index}]: content must be a non-empty string"
            ));
        }
    };
    let status = match fields.get("status") {
        Some(DataValue::String(value)) if !value.trim().is_empty() => value.clone(),
        Some(DataValue::String(_)) | Some(_) | None => {
            return Err(format!(
                "todos[{index}]: status must be pending | in_progress | completed"
            ));
        }
    };
    let active_form = match fields.get("activeForm") {
        Some(DataValue::String(value)) if !value.trim().is_empty() => value.clone(),
        Some(DataValue::String(_)) | Some(_) | None => {
            return Err(format!(
                "todos[{index}]: activeForm must be a non-empty string"
            ));
        }
    };

    Ok(TodoItem {
        parent_specified: fields.contains_key("parent"),
        id: match fields.get("id") {
            None => None,
            Some(DataValue::String(id)) => Some(id.clone()),
            _ => return Err(format!("todos[{index}]: id must be a string")),
        },
        parent: match fields.get("parent") {
            None | Some(DataValue::Null) => None,
            Some(DataValue::String(parent)) => Some(parent.clone()),
            _ => return Err(format!("todos[{index}]: parent must be a string or null")),
        },
        content,
        status,
        active_form,
    })
}

pub(super) fn todo_file_path_from_root(
    workspace_root: &Path,
    tool_name: &str,
) -> Result<PathBuf, String> {
    let canonical_root = canonical_workspace_root(workspace_root, tool_name)?;
    Ok(canonical_root
        .join(TODO_DIRECTORY_NAME)
        .join(TODO_FILE_NAME))
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub(crate) struct AgentTodos {
    pub(crate) tasks: Vec<TodoItem>,
    pub(crate) revision: String,
}

static AGENT_TODO_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Workspace-relative path of one agent's task file. Hex-encoding `id` byte by
/// byte keeps it a single path component (never `..` or a separator), so only
/// the fixed `.animaos-swarm/agent-tasks` prefix can introduce a symlink escape.
fn agent_todo_relative_path(id: &str) -> String {
    let filename: String = id.bytes().map(|byte| format!("{byte:02x}")).collect();
    format!("{TODO_DIRECTORY_NAME}/agent-tasks/{filename}.json")
}

fn agent_todo_path(root: Option<&Path>, id: &str) -> Result<PathBuf, String> {
    let root = workspace_root_path("agent tasks", root)?;
    // The resolver performs no write. Reuse its ancestor/target redirect
    // checks for reads too, including missing and dangling targets.
    resolve_workspace_write_path(&root, &agent_todo_relative_path(id), "agent tasks")
}

fn agent_todo_bytes_at(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    // Bound the actual read, including a file that grows after opening. Larger
    // legacy files are preserved and reported unavailable, never reset.
    const MAX_CANONICAL_READ: u64 = 2 * 1024 * 1024;
    match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_CANONICAL_READ + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("Could not read agent tasks: {error}"))?;
            if bytes.len() as u64 > MAX_CANONICAL_READ {
                return Err(
                    "Saved task file exceeds the bounded read limit; original bytes preserved"
                        .into(),
                );
            }
            Ok(bytes)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(b"[]".to_vec()),
        Err(error) => Err(format!("Could not read agent tasks: {error}")),
    }
}

fn decode_agent_todos(bytes: &[u8]) -> Result<AgentTodos, String> {
    use std::hash::{Hash, Hasher};
    let tasks: Vec<TodoItem> =
        serde_json::from_slice(bytes).map_err(|e| format!("Could not decode agent tasks: {e}"))?;
    validate_todo_items(&tasks)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    Ok(AgentTodos {
        tasks,
        revision: format!("{:016x}", hash.finish()),
    })
}

fn agent_todos_at(path: &Path) -> Result<AgentTodos, String> {
    decode_agent_todos(&agent_todo_bytes_at(path)?)
}

pub(crate) fn read_agent_todos(root: Option<&Path>, id: &str) -> Result<AgentTodos, String> {
    let _lock = AGENT_TODO_LOCK
        .lock()
        .map_err(|_| "Task store unavailable")?;
    agent_todos_at(&agent_todo_path(root, id)?)
}

/// Compatibility only for the old complete-list tool on legacy data. ID-aware
/// tools/data require an actual complete read; active reminders never grant CAS.
pub(crate) fn startup_revision(
    root: Option<&Path>,
    id: &str,
    config: &anima_core::AgentConfig,
) -> Option<String> {
    if !config.allows_tool("todo_write") || config.allows_tool("todo_list") {
        return None;
    }
    read_agent_todos(root, id)
        .ok()
        .filter(|snapshot| snapshot.tasks.iter().all(|task| task.id.is_none()))
        .map(|snapshot| snapshot.revision)
}

pub(crate) fn write_agent_todos(
    root: Option<&Path>,
    id: &str,
    tasks: &[TodoItem],
    expected_revision: Option<&str>,
) -> Result<AgentTodos, String> {
    use std::io::Write;
    validate_todo_items(tasks)?;
    let _lock = AGENT_TODO_LOCK
        .lock()
        .map_err(|_| "Task store unavailable")?;
    // Spec §14: every workspace writer must use the hardened write path. This
    // atomic write can't go through `write_workspace_bytes` directly (that helper
    // truncates in place), so its two checks are replicated by hand: resolve and
    // validate the target up front, then re-verify the canonical parent stays
    // inside the workspace after `create_dir_all` (mirrors `write_workspace_bytes`).
    let workspace_root = workspace_root_path("agent tasks", root)?;
    let relative_path = agent_todo_relative_path(id);
    let path = resolve_workspace_write_path(&workspace_root, &relative_path, "agent tasks")?;
    // Reading/containment failures still refuse. An explicit unconditional
    // owner save may repair undecodable data; native tools always supply an
    // observed revision and can never use that repair path.
    let bytes = agent_todo_bytes_at(&path)?;
    let current = match decode_agent_todos(&bytes) {
        Ok(current) => Some(current),
        Err(error) if expected_revision.is_some() => return Err(error),
        Err(_) => None,
    };
    if let Some(expected) = expected_revision {
        if current
            .as_ref()
            .is_none_or(|current| current.revision != expected)
        {
            return Err("Tasks changed. Refresh before saving again.".into());
        }
    }
    if let Some(current) = current {
        if current.tasks.iter().any(|t| t.id.is_some()) && tasks.iter().any(|t| t.id.is_none()) {
            return Err("Saved tasks have identities and hierarchy. Nothing was saved: read the full ID-aware list and preserve every id and parent, or use todo_list with an explicit merge.".into());
        }
        for old in current.tasks.iter().filter(|old| old.parent.is_some()) {
            if tasks
                .iter()
                .any(|new| new.id == old.id && new.parent.is_none() && !new.parent_specified)
            {
                return Err("Nothing was saved: retain the saved parent field or explicitly send parent:null to clear it.".into());
            }
        }
    }
    let parent = path.parent().expect("task parent");
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let canonical_root = canonical_workspace_root(&workspace_root, "agent tasks")?;
    let canonical_parent = parent.canonicalize().map_err(|error| {
        format!("agent tasks path could not be resolved: {relative_path} ({error})")
    })?;
    ensure_path_within_workspace(
        &canonical_root,
        &canonical_parent,
        "agent tasks",
        &relative_path,
    )?;
    let bytes = serde_json::to_vec_pretty(tasks).map_err(|e| e.to_string())?;
    atomicwrites::AtomicFile::new(&path, atomicwrites::AllowOverwrite)
        .write(|file| file.write_all(&bytes))
        .map_err(|e| e.to_string())?;
    agent_todos_at(&path)
}

#[cfg(test)]
mod agent_tests {
    use super::*;

    #[test]
    fn per_agent_tasks_are_isolated_and_stale_edits_do_not_overwrite_tool_updates() {
        let root = std::env::temp_dir().join(format!("agent-tasks-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let empty = read_agent_todos(Some(&root), "one").unwrap();
        let task = TodoItem {
            parent_specified: false,
            id: None,
            parent: None,
            content: "Research".into(),
            status: "pending".into(),
            active_form: "Researching".into(),
        };
        let saved =
            write_agent_todos(Some(&root), "one", &[task.clone()], Some(&empty.revision)).unwrap();
        assert_eq!(
            read_agent_todos(Some(&root), "one").unwrap().tasks,
            vec![task.clone()]
        );
        assert!(read_agent_todos(Some(&root), "two")
            .unwrap()
            .tasks
            .is_empty());
        let completed = TodoItem {
            status: "completed".into(),
            ..task
        };
        write_agent_todos(Some(&root), "one", &[completed.clone()], None).unwrap();
        assert!(write_agent_todos(Some(&root), "one", &[], Some(&saved.revision)).is_err());
        assert_eq!(
            read_agent_todos(Some(&root), "one").unwrap().tasks,
            vec![completed]
        );
        assert!(agent_todo_path(Some(&root), "../escape")
            .unwrap()
            .starts_with(root.canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }

    use anima_core::TaskStatus;
    use std::sync::Arc;

    fn todo_agent(id: &str) -> AgentState {
        AgentState {
            id: id.into(),
            name: "todo-agent".into(),
            status: anima_core::AgentStatus::Idle,
            config: anima_core::AgentConfig {
                name: "todo-agent".into(),
                model: "test".into(),
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
            token_usage: anima_core::TokenUsage::default(),
        }
    }

    fn todo_context(root: &Path) -> ToolExecutionContext {
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

    fn user_message() -> Message {
        Message {
            id: "todo-message".into(),
            agent_id: "agent-cas".into(),
            room_id: "room-a".into(),
            content: Content::default(),
            role: anima_core::MessageRole::User,
            created_at_ms: 1,
        }
    }

    fn write_call(items: &[&str]) -> ToolCall {
        ToolCall {
            id: "todo-write".into(),
            name: "todo_write".into(),
            args: std::collections::BTreeMap::from([(
                "todos".to_string(),
                DataValue::Array(
                    items
                        .iter()
                        .map(|content| {
                            DataValue::Object(std::collections::BTreeMap::from([
                                ("content".to_string(), DataValue::String((*content).into())),
                                ("status".to_string(), DataValue::String("pending".into())),
                                (
                                    "activeForm".to_string(),
                                    DataValue::String(format!("Doing {content}")),
                                ),
                            ]))
                        })
                        .collect(),
                ),
            )]),
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[tokio::test]
    async fn todo_write_is_compare_and_swap_across_concurrent_runs() {
        let root = temp_root("agent-todo-cas");
        let baseline = read_agent_todos(Some(&root), "agent-cas").unwrap().revision;
        let first = todo_context(&root).with_todo_baseline(Some(baseline.clone()));
        let second = todo_context(&root).with_todo_baseline(Some(baseline));

        let saved = execute_todo_write(
            second,
            todo_agent("agent-cas"),
            user_message(),
            write_call(&["From room B"]),
        )
        .await;
        assert_eq!(saved.status, TaskStatus::Success);

        let conflict = execute_todo_write(
            first.clone(),
            todo_agent("agent-cas"),
            user_message(),
            write_call(&["From room A"]),
        )
        .await;
        assert_eq!(conflict.status, TaskStatus::Error);
        let message = conflict.error.unwrap();
        assert!(
            message
                .starts_with("Tasks changed since this run last saw them, so nothing was saved."),
            "{message}"
        );
        assert!(
            message.contains("[ ] 1. [pending] From room B"),
            "{message}"
        );
        assert!(
            message.ends_with("call todo_write again with the complete list."),
            "{message}"
        );
        assert_eq!(
            read_agent_todos(Some(&root), "agent-cas").unwrap().tasks[0].content,
            "From room B"
        );

        let merged = execute_todo_write(
            first,
            todo_agent("agent-cas"),
            user_message(),
            write_call(&["From room B", "From room A"]),
        )
        .await;
        assert_eq!(merged.status, TaskStatus::Success);
        assert_eq!(
            merged.data.unwrap().text,
            "Todos updated (0 completed, 0 in progress, 2 pending). Proceed with current tasks."
        );
        assert_eq!(
            read_agent_todos(Some(&root), "agent-cas")
                .unwrap()
                .tasks
                .len(),
            2
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn completion_review_todo_save_returns_complete_objects_and_keeps_revision_checks() {
        let root = temp_root("completion-todo-reply");
        let baseline = read_agent_todos(Some(&root), "agent-cas").unwrap().revision;
        let context = todo_context(&root)
            .with_todo_baseline(Some(baseline))
            .with_completion_review(Some(anima_core::CompletionReview::default()));
        let saved = execute_todo_write(
            context,
            todo_agent("agent-cas"),
            user_message(),
            write_call(&["Inspect", "Verify"]),
        )
        .await;
        assert_eq!(saved.status, TaskStatus::Success);
        let response = saved.data.unwrap().text;
        let objects: Vec<TodoItem> =
            serde_json::from_str(response.lines().last().unwrap()).unwrap();
        assert_eq!(
            objects,
            read_agent_todos(Some(&root), "agent-cas").unwrap().tasks
        );
        assert_eq!(objects[1].active_form, "Doing Verify");
        let stale = todo_context(&root)
            .with_todo_baseline(Some("stale".into()))
            .with_completion_review(Some(anima_core::CompletionReview::default()));
        let refused = execute_todo_write(
            stale,
            todo_agent("agent-cas"),
            user_message(),
            write_call(&["Replace"]),
        )
        .await;
        assert_eq!(refused.status, TaskStatus::Error);
        assert_eq!(
            read_agent_todos(Some(&root), "agent-cas").unwrap().tasks,
            objects
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn fp03_opted_reads_and_conflicts_return_complete_preserved_task_objects() {
        let private = tempfile::tempdir().unwrap();
        let root = private.path();
        let agent = "fp03-agent";
        let original = TodoItem {
            parent_specified: false,
            id: None,
            parent: None,
            content: "Preserve the exact task".into(),
            status: "in_progress".into(),
            active_form: "Inspecting the original result".into(),
        };
        let first = write_agent_todos(Some(root), agent, &[original.clone()], None).unwrap();
        let path = agent_todo_path(Some(root), agent).unwrap();
        let bytes_before_read = fs::read(&path).unwrap();
        let context = todo_context(root)
            .with_completion_review(Some(anima_core::CompletionReview::default()));
        let read = execute_todo_read(
            context.clone(),
            todo_agent(agent),
            user_message(),
            ToolCall {
                id: "read".into(),
                name: "todo_read".into(),
                args: Default::default(),
            },
        )
        .await;
        assert_eq!(read.status, TaskStatus::Success);
        let read_text = read.data.unwrap().text;
        let objects: Vec<TodoItem> = serde_json::from_str(read_text.lines().last().unwrap())
            .expect("opted reads must return all fields required for a lossless next write");
        assert_eq!(objects, vec![original.clone()]);
        assert_eq!(fs::read(&path).unwrap(), bytes_before_read);
        assert_eq!(
            context.todo_revision.lock().unwrap().as_ref(),
            Some(&first.revision)
        );
        let changed = TodoItem {
            content: "Owner added a clarification".into(),
            ..original.clone()
        };
        let latest = write_agent_todos(Some(root), agent, &[changed.clone()], None).unwrap();
        let conflict = execute_todo_write(
            context.clone(),
            todo_agent(agent),
            user_message(),
            write_call(&["obsolete replacement"]),
        )
        .await;
        assert_eq!(conflict.status, TaskStatus::Error);
        let conflict_text = conflict.error.unwrap();
        let objects: Vec<TodoItem> = serde_json::from_str(conflict_text.lines().last().unwrap())
            .expect("conflict recovery must preserve the activeForm and description");
        assert_eq!(objects, vec![changed.clone()]);
        assert_eq!(
            read_agent_todos(Some(root), agent).unwrap().tasks,
            vec![changed]
        );
        assert_eq!(
            context.todo_revision.lock().unwrap().as_ref(),
            Some(&latest.revision)
        );
    }

    #[tokio::test]
    async fn fp03_oversized_and_malformed_reads_preserve_exact_saved_bytes() {
        let private = tempfile::tempdir().unwrap();
        let root = private.path();
        let agent = "fp03-large";
        let tasks: Vec<TodoItem> = (0..80)
            .map(|index| TodoItem {
                parent_specified: false,
                id: None,
                parent: None,
                content: format!("Task {index}: {}", "x".repeat(400)),
                status: "pending".into(),
                active_form: format!("Inspecting task {index}"),
            })
            .collect();
        write_agent_todos(Some(root), agent, &tasks, None).unwrap();
        let path = agent_todo_path(Some(root), agent).unwrap();
        let bytes = fs::read(&path).unwrap();
        let context = todo_context(root)
            .with_completion_review(Some(anima_core::CompletionReview::default()));
        let read_call = || ToolCall {
            id: "read-large".into(),
            name: "todo_read".into(),
            args: Default::default(),
        };
        let read = execute_todo_read(
            context.clone(),
            todo_agent(agent),
            user_message(),
            read_call(),
        )
        .await;
        assert_eq!(read.status, TaskStatus::Success);
        let reply = read.data.unwrap().text;
        assert!(reply.contains("Complete saved task objects omitted"));
        assert!(!reply.contains("Saved task objects (data;"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(
            context.todo_revision.lock().unwrap().is_none(),
            "omitted full objects cannot authorize a replacement"
        );
        let malformed = b"{\"tasks\": [invalid exact bytes\r\n";
        fs::write(&path, malformed).unwrap();
        let refused = execute_todo_read(
            context.clone(),
            todo_agent(agent),
            user_message(),
            read_call(),
        )
        .await;
        assert_eq!(refused.status, TaskStatus::Error);
        assert_eq!(fs::read(&path).unwrap(), malformed);
        assert!(context.todo_revision.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn fp03_legacy_reads_keep_their_existing_text_contract() {
        let private = tempfile::tempdir().unwrap();
        let root = private.path();
        write_agent_todos(
            Some(root),
            "fp03-legacy",
            &[TodoItem {
                parent_specified: false,
                id: None,
                parent: None,
                content: "Legacy task".into(),
                status: "pending".into(),
                active_form: "Doing legacy task".into(),
            }],
            None,
        )
        .unwrap();
        let result = execute_todo_read(
            todo_context(root),
            todo_agent("fp03-legacy"),
            user_message(),
            ToolCall {
                id: "read".into(),
                name: "todo_read".into(),
                args: Default::default(),
            },
        )
        .await;
        assert_eq!(result.data.unwrap().text, "[ ] 1. [pending] Legacy task");
    }

    #[tokio::test]
    async fn todo_read_refreshes_the_revision_a_run_writes_against() {
        let root = temp_root("agent-todo-read");
        let context = todo_context(&root).with_todo_baseline(Some("stale-revision".into()));

        let read = execute_todo_read(
            context.clone(),
            todo_agent("agent-read"),
            user_message(),
            ToolCall {
                id: "todo-read".into(),
                name: "todo_read".into(),
                args: std::collections::BTreeMap::new(),
            },
        )
        .await;
        assert_eq!(read.data.unwrap().text, "No todos set.");

        let written = execute_todo_write(
            context,
            todo_agent("agent-read"),
            user_message(),
            write_call(&["Plan"]),
        )
        .await;
        assert_eq!(written.status, TaskStatus::Success);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn todo_write_without_a_baseline_preserves_the_existing_list() {
        let root = temp_root("agent-todo-blind");
        let path = agent_todo_path(Some(&root), "agent-blind").unwrap();
        let original = br#"[
            {"content":"Existing task","status":"pending","activeForm":"Doing existing task"}
        ]"#;
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, original).unwrap();
        let context = todo_context(&root);

        let written = execute_todo_write(
            context.clone(),
            todo_agent("agent-blind"),
            user_message(),
            write_call(&["Plan"]),
        )
        .await;
        let after = fs::read(&path).unwrap();
        let baseline = context.todo_revision.lock().unwrap().clone();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(written.status, TaskStatus::Error);
        let error = written.error.unwrap();
        assert!(error.contains("nothing was saved"), "{error}");
        assert!(error.contains("todo_read"), "{error}");
        assert!(
            error.contains("merge") || error.contains("Merge"),
            "{error}"
        );
        assert!(!error.starts_with(TASKS_CHANGED_PREFIX), "{error}");
        assert_eq!(after.as_slice(), original);
        assert_eq!(baseline, None);
    }

    #[tokio::test]
    async fn failed_todo_read_does_not_authorize_overwriting_malformed_bytes() {
        let root = temp_root("agent-todo-malformed");
        let path = agent_todo_path(Some(&root), "agent-malformed").unwrap();
        let original = b"{ unfinished tasks: \xff\r\n";
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, original).unwrap();
        let initial = read_agent_todos(Some(&root), "agent-malformed");
        let context = todo_context(&root).with_todo_baseline(
            initial
                .as_ref()
                .ok()
                .map(|snapshot| snapshot.revision.clone()),
        );

        let read = execute_todo_read(
            context.clone(),
            todo_agent("agent-malformed"),
            user_message(),
            ToolCall {
                id: "todo-read".into(),
                name: "todo_read".into(),
                args: std::collections::BTreeMap::new(),
            },
        )
        .await;
        let after_read = fs::read(&path).unwrap();
        let baseline_after_read = context.todo_revision.lock().unwrap().clone();
        let written = execute_todo_write(
            context.clone(),
            todo_agent("agent-malformed"),
            user_message(),
            write_call(&["Plan"]),
        )
        .await;
        let after_write = fs::read(&path).unwrap();
        let baseline_after_write = context.todo_revision.lock().unwrap().clone();
        fs::remove_dir_all(root).unwrap();

        assert!(initial.is_err());
        assert_eq!(read.status, TaskStatus::Error);
        assert!(read
            .error
            .unwrap()
            .starts_with("Could not decode agent tasks:"));
        assert_eq!(after_read.as_slice(), original);
        assert_eq!(baseline_after_read, None);
        assert_eq!(written.status, TaskStatus::Error);
        assert_eq!(after_write.as_slice(), original);
        assert_eq!(baseline_after_write, None);
    }

    #[tokio::test]
    async fn repaired_tasks_require_an_explicit_read_before_a_merged_write() {
        let root = temp_root("agent-todo-repaired");
        let path = agent_todo_path(Some(&root), "agent-repaired").unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"unfinished tasks").unwrap();
        let initial = read_agent_todos(Some(&root), "agent-repaired");
        let context = todo_context(&root).with_todo_baseline(
            initial
                .as_ref()
                .ok()
                .map(|snapshot| snapshot.revision.clone()),
        );
        let other_room_task = TodoItem {
            parent_specified: false,
            id: None,
            parent: None,
            content: "From room B".into(),
            status: "pending".into(),
            active_form: "Doing From room B".into(),
        };
        let repaired = write_agent_todos(
            Some(&root),
            "agent-repaired",
            &[other_room_task.clone()],
            None,
        )
        .unwrap();
        let repaired_bytes = fs::read(&path).unwrap();

        let refused = execute_todo_write(
            context.clone(),
            todo_agent("agent-repaired"),
            user_message(),
            write_call(&["From room A"]),
        )
        .await;
        let after_refusal = fs::read(&path).unwrap();
        let baseline_after_refusal = context.todo_revision.lock().unwrap().clone();
        let read = execute_todo_read(
            context.clone(),
            todo_agent("agent-repaired"),
            user_message(),
            ToolCall {
                id: "todo-read".into(),
                name: "todo_read".into(),
                args: std::collections::BTreeMap::new(),
            },
        )
        .await;
        let baseline_after_read = context.todo_revision.lock().unwrap().clone();
        let merged = execute_todo_write(
            context.clone(),
            todo_agent("agent-repaired"),
            user_message(),
            write_call(&["From room B", "From room A"]),
        )
        .await;
        let saved = read_agent_todos(Some(&root), "agent-repaired").unwrap();
        let baseline_after_write = context.todo_revision.lock().unwrap().clone();
        fs::remove_dir_all(root).unwrap();

        assert!(initial.is_err());
        assert_eq!(refused.status, TaskStatus::Error);
        assert_eq!(after_refusal, repaired_bytes);
        assert_eq!(baseline_after_refusal, None);
        assert_eq!(read.status, TaskStatus::Success);
        assert_eq!(read.data.unwrap().text, "[ ] 1. [pending] From room B");
        assert_eq!(baseline_after_read, Some(repaired.revision));
        assert_eq!(merged.status, TaskStatus::Success);
        assert_eq!(saved.tasks[0], other_room_task);
        assert_eq!(saved.tasks[1].content, "From room A");
        assert_eq!(saved.tasks.len(), 2);
        assert_eq!(baseline_after_write, Some(saved.revision));
    }

    #[tokio::test]
    async fn missing_task_file_read_supplies_a_revision_for_the_first_write() {
        let root = temp_root("agent-todo-first-read");
        let path = agent_todo_path(Some(&root), "agent-first-read").unwrap();
        let empty = read_agent_todos(Some(&root), "agent-first-read").unwrap();
        let context = todo_context(&root);
        let read = execute_todo_read(
            context.clone(),
            todo_agent("agent-first-read"),
            user_message(),
            ToolCall {
                id: "todo-read".into(),
                name: "todo_read".into(),
                args: std::collections::BTreeMap::new(),
            },
        )
        .await;
        let baseline_after_read = context.todo_revision.lock().unwrap().clone();
        let file_exists_after_read = path.exists();
        let written = execute_todo_write(
            context.clone(),
            todo_agent("agent-first-read"),
            user_message(),
            write_call(&["Plan"]),
        )
        .await;
        let saved = read_agent_todos(Some(&root), "agent-first-read").unwrap();
        let baseline_after_write = context.todo_revision.lock().unwrap().clone();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(read.status, TaskStatus::Success);
        assert_eq!(read.data.unwrap().text, "No todos set.");
        assert!(empty.tasks.is_empty());
        assert_eq!(baseline_after_read, Some(empty.revision));
        assert!(!file_exists_after_read);
        assert_eq!(written.status, TaskStatus::Success);
        assert_eq!(saved.tasks.len(), 1);
        assert_eq!(saved.tasks[0].content, "Plan");
        assert_eq!(baseline_after_write, Some(saved.revision));
    }

    #[tokio::test]
    async fn blind_first_todo_write_does_not_create_the_task_directory() {
        let root = temp_root("agent-todo-blind-first");
        let context = todo_context(&root);
        let written = execute_todo_write(
            context.clone(),
            todo_agent("agent-blind-first"),
            user_message(),
            write_call(&["Plan"]),
        )
        .await;
        let task_directory_exists = root.join(TODO_DIRECTORY_NAME).exists();
        let baseline = context.todo_revision.lock().unwrap().clone();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(written.status, TaskStatus::Error);
        assert!(!task_directory_exists);
        assert_eq!(baseline, None);
    }

    #[test]
    fn explicit_unconditional_agent_writer_can_replace_an_existing_list() {
        let root = temp_root("agent-todo-unconditional");
        let original = TodoItem {
            parent_specified: false,
            id: None,
            parent: None,
            content: "Original".into(),
            status: "pending".into(),
            active_form: "Doing Original".into(),
        };
        let initial =
            write_agent_todos(Some(&root), "agent-unconditional", &[original], None).unwrap();
        let replacement = TodoItem {
            parent_specified: false,
            id: None,
            parent: None,
            content: "Explicit replacement".into(),
            status: "completed".into(),
            active_form: "Doing explicit replacement".into(),
        };
        let updated = write_agent_todos(
            Some(&root),
            "agent-unconditional",
            &[replacement.clone()],
            None,
        )
        .unwrap();
        let saved = read_agent_todos(Some(&root), "agent-unconditional").unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(saved.tasks, vec![replacement]);
        assert_eq!(saved.revision, updated.revision);
        assert_ne!(saved.revision, initial.revision);
    }

    #[cfg(unix)]
    #[test]
    fn write_agent_todos_rejects_symlinked_animaos_swarm_directory_pointing_outside() {
        let sandbox = temp_root("agent-todo-symlink-sandbox");
        let workspace = sandbox.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let outside = sandbox.join("outside");
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, workspace.join(".animaos-swarm")).unwrap();

        let task = TodoItem {
            parent_specified: false,
            id: None,
            parent: None,
            content: "Leak".into(),
            status: "pending".into(),
            active_form: "Leaking".into(),
        };
        let error = write_agent_todos(Some(&workspace), "agent-x", &[task], None)
            .expect_err("symlinked .animaos-swarm directory must be rejected");
        assert_eq!(
            error,
            format!(
                "agent tasks path escapes workspace root: .animaos-swarm/agent-tasks/{}.json",
                "agent-x"
                    .bytes()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            )
        );
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn fp03_review_canonical_reads_refuse_redirected_task_directories() {
        for redirect_parent in [true, false] {
            let private = tempfile::tempdir().unwrap();
            let workspace = private.path().join("workspace");
            let outside = private.path().join("outside");
            fs::create_dir_all(&workspace).unwrap();
            fs::create_dir_all(&outside).unwrap();
            let id = "synthetic-outside-owner";
            let relative = PathBuf::from(agent_todo_relative_path(id));
            let outside_file = if redirect_parent {
                outside.join(relative.strip_prefix(TODO_DIRECTORY_NAME).unwrap())
            } else {
                outside.join(relative.file_name().unwrap())
            };
            fs::create_dir_all(outside_file.parent().unwrap()).unwrap();
            let original = br#"[{"content":"Synthetic outside data must not enter context","status":"pending","activeForm":"Checking boundary"}]"#;
            fs::write(&outside_file, original).unwrap();
            let link = if redirect_parent {
                workspace.join(TODO_DIRECTORY_NAME)
            } else {
                fs::create_dir_all(workspace.join(TODO_DIRECTORY_NAME)).unwrap();
                workspace.join(TODO_DIRECTORY_NAME).join("agent-tasks")
            };
            #[cfg(windows)]
            {
                let made = std::process::Command::new("cmd.exe")
                    .args(["/C", "mklink", "/J"])
                    .arg(&link)
                    .arg(&outside)
                    .output()
                    .unwrap();
                assert!(made.status.success(), "synthetic junction setup failed");
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            let read = read_agent_todos(Some(&workspace), id);
            // Remove only the synthetic link before checking the outcome.
            #[cfg(windows)]
            fs::remove_dir(&link).unwrap();
            #[cfg(unix)]
            fs::remove_file(&link).unwrap();
            assert_eq!(fs::read(&outside_file).unwrap(), original);
            assert!(
                read.is_err(),
                "canonical task reads must refuse an escaping directory redirect"
            );
        }
    }
}
