//! Portable task identity, merge and active-context semantics. Hosts own task
//! scope, revision checks and persistence; this module never grants authority.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub const MAX_TASKS: usize = 256;
pub const MAX_TASK_ID_BYTES: usize = 128;
pub const MAX_TASK_TEXT_CHARS: usize = 4_000;
pub const MAX_TASK_PAYLOAD_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

impl TaskState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskRecord {
    pub id: String,
    pub content: String,
    pub status: TaskState,
    pub active_form: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ParentChange {
    #[default]
    Keep,
    Set(Option<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskPatch {
    pub id: String,
    pub content: Option<String>,
    pub status: Option<TaskState>,
    pub active_form: Option<String>,
    pub parent: ParentChange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskError {
    InvalidId,
    InvalidText,
    DuplicateId,
    UnknownParent,
    Cycle,
    Budget,
    IncompleteNewTask,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveTaskContext {
    /// Complete JSON data, or absent when nothing is active or the whole
    /// required lineage cannot fit. Absence never means the saved list is empty.
    pub json: Option<String>,
    pub omitted_for_budget: bool,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_TASK_ID_BYTES
        && id.trim() == id
        && !id.chars().any(char::is_control)
}

fn valid_text(text: &str) -> bool {
    text.chars().take(MAX_TASK_TEXT_CHARS + 1).count() <= MAX_TASK_TEXT_CHARS
        && !text.trim().is_empty()
}

pub fn validate(tasks: &[TaskRecord]) -> Result<(), TaskError> {
    if tasks.len() > MAX_TASKS {
        return Err(TaskError::Budget);
    }
    let mut by_id = HashMap::new();
    for task in tasks {
        if !valid_id(&task.id) || task.parent.as_deref().is_some_and(|id| !valid_id(id)) {
            return Err(TaskError::InvalidId);
        }
        if !valid_text(&task.content) || !valid_text(&task.active_form) {
            return Err(TaskError::InvalidText);
        }
        if by_id.insert(task.id.as_str(), task).is_some() {
            return Err(TaskError::DuplicateId);
        }
    }
    for task in tasks {
        let mut seen = HashSet::from([task.id.as_str()]);
        let mut next = task.parent.as_deref();
        while let Some(id) = next {
            if !seen.insert(id) {
                return Err(TaskError::Cycle);
            }
            next = by_id
                .get(id)
                .ok_or(TaskError::UnknownParent)?
                .parent
                .as_deref();
        }
    }
    if serde_json::to_vec(tasks)
        .map_err(|_| TaskError::Budget)?
        .len()
        > MAX_TASK_PAYLOAD_BYTES
    {
        return Err(TaskError::Budget);
    }
    Ok(())
}

/// Replacements and merges are proposed values; only the host can CAS/save.
pub fn merge(tasks: &[TaskRecord], patches: &[TaskPatch]) -> Result<Vec<TaskRecord>, TaskError> {
    validate(tasks)?;
    if patches.len() > MAX_TASKS {
        return Err(TaskError::Budget);
    }
    let mut result = tasks.to_vec();
    let mut positions: HashMap<String, usize> = result
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.clone(), i))
        .collect();
    let mut changed = HashSet::new();
    for patch in patches {
        if !valid_id(&patch.id) {
            return Err(TaskError::InvalidId);
        }
        if !changed.insert(patch.id.as_str()) {
            return Err(TaskError::DuplicateId);
        }
        if let Some(&position) = positions.get(&patch.id) {
            let task = &mut result[position];
            if let Some(value) = &patch.content {
                task.content = value.clone();
            }
            if let Some(value) = patch.status {
                task.status = value;
            }
            if let Some(value) = &patch.active_form {
                task.active_form = value.clone();
            }
            if let ParentChange::Set(value) = &patch.parent {
                task.parent = value.clone();
            }
        } else {
            if result.len() == MAX_TASKS {
                return Err(TaskError::Budget);
            }
            let task = TaskRecord {
                id: patch.id.clone(),
                content: patch.content.clone().ok_or(TaskError::IncompleteNewTask)?,
                status: patch.status.ok_or(TaskError::IncompleteNewTask)?,
                active_form: patch
                    .active_form
                    .clone()
                    .ok_or(TaskError::IncompleteNewTask)?,
                parent: match &patch.parent {
                    ParentChange::Keep => None,
                    ParentChange::Set(value) => value.clone(),
                },
            };
            positions.insert(task.id.clone(), result.len());
            result.push(task);
        }
    }
    validate(&result)?;
    Ok(result)
}

pub fn active_context(
    tasks: &[TaskRecord],
    max_bytes: usize,
) -> Result<ActiveTaskContext, TaskError> {
    validate(tasks)?;
    let by_id: HashMap<&str, &TaskRecord> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut keep = HashSet::new();
    for task in tasks.iter().filter(|t| t.status.is_active()) {
        let mut next = Some(task.id.as_str());
        while let Some(id) = next {
            if !keep.insert(id) {
                break;
            }
            next = by_id[id].parent.as_deref();
        }
    }
    if keep.is_empty() {
        return Ok(ActiveTaskContext {
            json: None,
            omitted_for_budget: false,
        });
    }
    let kept: Vec<&TaskRecord> = tasks
        .iter()
        .filter(|t| keep.contains(t.id.as_str()))
        .collect();
    let json = serde_json::to_string(&kept).map_err(|_| TaskError::Budget)?;
    if json.len() > max_bytes.min(MAX_TASK_PAYLOAD_BYTES) {
        return Ok(ActiveTaskContext {
            json: None,
            omitted_for_budget: true,
        });
    }
    Ok(ActiveTaskContext {
        json: Some(json),
        omitted_for_budget: false,
    })
}

/// Stable only for this exact host-observed revision and item position.
pub fn provisional_id(revision: &str, index: usize) -> Result<String, TaskError> {
    if revision.trim().is_empty() || revision.len() > MAX_TASK_ID_BYTES || index >= MAX_TASKS {
        return Err(TaskError::InvalidId);
    }
    let mut hash = Sha256::new();
    hash.update(b"anima-structured-task-provisional-v1\0");
    hash.update((revision.len() as u64).to_le_bytes());
    hash.update(revision.as_bytes());
    hash.update((index as u64).to_le_bytes());
    let digest = hash.finalize();
    Ok(format!(
        "task_{}",
        digest[..20]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(id: &str, parent: Option<&str>, status: TaskState) -> TaskRecord {
        TaskRecord {
            id: id.into(),
            content: format!("Exact task {id}"),
            active_form: format!("Working on {id}"),
            status,
            parent: parent.map(str::to_owned),
        }
    }
    fn patch(id: &str) -> TaskPatch {
        TaskPatch {
            id: id.into(),
            content: None,
            status: None,
            active_form: None,
            parent: ParentChange::Keep,
        }
    }

    #[test]
    fn fp03_structured_merge_preserves_omitted_fields_and_order_and_appends_complete_new_tasks() {
        let before = vec![
            task("root", None, TaskState::Completed),
            task("child", Some("root"), TaskState::InProgress),
        ];
        let mut cancel = patch("child");
        cancel.status = Some(TaskState::Cancelled);
        let new = task("next", None, TaskState::Pending);
        let append = TaskPatch {
            id: new.id.clone(),
            content: Some(new.content.clone()),
            status: Some(new.status),
            active_form: Some(new.active_form.clone()),
            parent: ParentChange::Keep,
        };
        let result = merge(&before, &[cancel, append]).unwrap();
        let mut expected = before.clone();
        expected[1].status = TaskState::Cancelled;
        expected.push(new);
        assert_eq!(result, expected);
        assert_eq!(
            before[1].status,
            TaskState::InProgress,
            "proposal must not mutate the saved input"
        );
        let mut clear = patch("child");
        clear.parent = ParentChange::Set(None);
        assert_eq!(merge(&result, &[clear]).unwrap()[1].parent, None);
    }

    #[test]
    fn fp03_structured_merge_refuses_invalid_patches_atomically() {
        let before = vec![
            task("a", None, TaskState::Pending),
            task("b", Some("a"), TaskState::Pending),
        ];
        let mut good = patch("b");
        good.content = Some("A valid proposed edit".into());
        let mut cycle = patch("a");
        cycle.parent = ParentChange::Set(Some("b".into()));
        let mut blank = patch("b");
        blank.active_form = Some(" ".into());
        let mut dangling = patch("b");
        dangling.parent = ParentChange::Set(Some("missing".into()));
        for bad in [cycle, blank, dangling, patch("new"), patch("bad id\n")] {
            assert!(merge(&before, &[good.clone(), bad]).is_err());
        }
        assert!(merge(&before, &[patch("a"), patch("a")]).is_err());
        assert_eq!(before[1].content, "Exact task b");
    }

    #[test]
    fn fp03_structured_validation_bounds_identity_text_parentage_and_payload() {
        let valid = vec![
            task("a", None, TaskState::Pending),
            task("b", Some("a"), TaskState::Completed),
        ];
        validate(&valid).unwrap();
        let mut duplicate = valid.clone();
        duplicate[1].id = "a".into();
        let mut missing = valid.clone();
        missing[1].parent = Some("missing".into());
        let mut cycle = valid.clone();
        cycle[0].parent = Some("b".into());
        let mut long = valid.clone();
        long[0].content = "x".repeat(MAX_TASK_TEXT_CHARS + 1);
        let mut whitespace = valid.clone();
        whitespace[0].id = " a".into();
        for bad in [duplicate, missing, cycle, long, whitespace] {
            assert!(validate(&bad).is_err());
        }
        let too_many: Vec<_> = (0..=MAX_TASKS)
            .map(|i| task(&i.to_string(), None, TaskState::Pending))
            .collect();
        assert_eq!(validate(&too_many), Err(TaskError::Budget));
        let huge: Vec<_> = (0..MAX_TASKS)
            .map(|i| {
                let mut value = task(&i.to_string(), None, TaskState::Pending);
                value.content = "界".repeat(MAX_TASK_TEXT_CHARS);
                value
            })
            .collect();
        assert_eq!(validate(&huge), Err(TaskError::Budget));
    }

    #[test]
    fn fp03_structured_context_keeps_active_ancestors_without_terminal_leaves() {
        let tasks = vec![
            task("root", None, TaskState::Completed),
            task("middle", Some("root"), TaskState::Cancelled),
            task("active", Some("middle"), TaskState::InProgress),
            task("done", None, TaskState::Completed),
            task("cancelled", None, TaskState::Cancelled),
        ];
        let context = active_context(&tasks, 16 * 1024).unwrap();
        assert!(!context.omitted_for_budget);
        let kept: Vec<TaskRecord> = serde_json::from_str(
            context
                .json
                .as_deref()
                .expect("active lineage must survive compression"),
        )
        .unwrap();
        assert_eq!(kept, tasks[..3]);
        assert!(!TaskState::Cancelled.is_active());
        assert_eq!(active_context(&tasks[3..], 16 * 1024).unwrap().json, None);
    }

    #[test]
    fn fp03_structured_context_reports_omission_without_inventing_empty_saved_state() {
        let tasks = vec![
            task("root", None, TaskState::Completed),
            task("active", Some("root"), TaskState::Pending),
        ];
        let exact = serde_json::to_string(&tasks).unwrap();
        assert_eq!(
            active_context(&tasks, exact.len()).unwrap().json.as_deref(),
            Some(exact.as_str())
        );
        let omitted = active_context(&tasks, exact.len() - 1).unwrap();
        assert_eq!(omitted.json, None);
        assert!(omitted.omitted_for_budget);
        assert_eq!(tasks[1].content, "Exact task active");
    }

    #[test]
    fn fp03_structured_provisional_ids_are_revision_and_position_bound() {
        let first = provisional_id("exact-revision", 0).unwrap();
        assert_eq!(first, provisional_id("exact-revision", 0).unwrap());
        assert_ne!(first, provisional_id("exact-revision", 1).unwrap());
        assert_ne!(first, provisional_id("changed-revision", 0).unwrap());
        assert!(first.starts_with("task_"));
        assert!(first.len() <= MAX_TASK_ID_BYTES);
        assert!(provisional_id("", 0).is_err());
        assert!(provisional_id("revision", MAX_TASKS).is_err());
    }
}
