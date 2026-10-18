//! Helper worktrees (`spawn_helper{,_async}` with `worktree: true`).
//!
//! A helper cannot run git, so the daemon does all git work through
//! `anima-worktree`: it creates the worktree before the helper's run, points
//! the helper's file tools at it (see [`run_root`]), inspects it when the run
//! ends, removes it when the helper changed nothing, and reports the rest to
//! the companion. Agent files, memory and attachments keep the global
//! workspace; only the run's tool root moves.
use std::path::{Component, Path, PathBuf};

use anima_core::{AgentConfig, DataValue};
use anima_worktree::{Finished, Worktree, Worktrees};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Helper config keys, written only by the daemon for one assignment.
pub(crate) const ROOT_KEY: &str = "workspaceRoot";
pub(crate) const WORKTREE_KEY: &str = "worktree";
/// Bound on the repositories remembered for the startup sweep.
pub(crate) const MAX_REMEMBERED_REPOS: usize = 32;
const MAX_SUMMARY_FILES: usize = 50;
const MAX_SUMMARY_DIFF_STAT: usize = 4 * 1024;
const MAX_NOTE_BYTES: usize = 2 * 1024;

/// One assignment's worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HelperWorktree {
    pub(crate) worktree: Worktree,
    /// The helper's tool root: the worktree, at the requested directory's
    /// offset inside the repository.
    pub(crate) root: PathBuf,
    /// The worktree path shown to the companion: workspace-relative when the
    /// worktree is inside the workspace.
    pub(crate) display_path: String,
    /// The report built when the helper's run ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) report: Option<Value>,
}

impl HelperWorktree {
    /// What the companion sees: the end-of-run report, or where the
    /// uninspected worktree is.
    pub(crate) fn public_json(&self, compact: bool) -> Value {
        let mut value = self.report.clone().unwrap_or_else(|| {
            json!({
                "path": self.display_path, "branch": self.worktree.branch,
                "baseCommit": self.worktree.base_commit, "removed": false,
                "inspected": false,
                "note": bounded(&format!("Not inspected yet. {}", review_note(self)), MAX_NOTE_BYTES),
            })
        });
        if compact {
            if let Some(object) = value.as_object_mut() {
                object.remove("diffStat");
                if let Some(Value::Array(files)) = object.get_mut("changedFiles") {
                    files.truncate(20);
                }
            }
        }
        value
    }
}

/// Resolves `repo` (workspace-relative; default ".") inside `workspace`.
fn repo_dir(workspace: &Path, repo: &str) -> Result<(PathBuf, PathBuf), String> {
    let repo = repo.trim();
    let repo = if repo.is_empty() { "." } else { repo };
    let relative = Path::new(repo);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err("repo must be a workspace-relative path without '..'".into());
    }
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "The workspace folder is unavailable".to_string())?;
    let dir = workspace
        .join(relative)
        .canonicalize()
        .map_err(|_| format!("repo {repo} is not a folder in the workspace"))?;
    if !dir.starts_with(&workspace) || !dir.is_dir() {
        return Err(format!("repo {repo} is not a folder in the workspace"));
    }
    Ok((workspace, dir))
}

/// Creates a worktree of the repository containing `repo` for one helper
/// assignment. Never silently falls back to the shared workspace.
pub(crate) async fn create(
    workspace: Option<PathBuf>,
    repo: String,
) -> Result<HelperWorktree, String> {
    let workspace = workspace.ok_or("Helper worktrees need a configured workspace folder")?;
    tokio::task::spawn_blocking(move || {
        let (workspace, dir) = repo_dir(&workspace, &repo)?;
        let name = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        let worktree = Worktrees::new()
            .create(&dir, &name)
            .map_err(|error| format!("Could not create a helper worktree: {error}"))?;
        let offset = dir
            .strip_prefix(&worktree.repo_root)
            .unwrap_or(Path::new(""));
        let root = worktree.path.join(offset);
        let display_path = match worktree.path.strip_prefix(&workspace) {
            Ok(relative) => relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
            Err(_) => worktree.path.display().to_string(),
        };
        Ok(HelperWorktree {
            worktree,
            root,
            display_path,
            report: None,
        })
    })
    .await
    .map_err(|_| "Helper worktree creation stopped unexpectedly".to_string())?
}

/// Inspects the worktree after the helper's run; removes it when it holds no
/// work. Returns the companion's report.
pub(crate) async fn finish(worktree: &HelperWorktree) -> Value {
    let record = worktree.worktree.clone();
    let finished = tokio::task::spawn_blocking(move || Worktrees::new().finish(&record))
        .await
        .unwrap_or_else(|_| Finished {
            inspection_failed: true,
            error: Some("inspection stopped unexpectedly".into()),
            ..Finished::default()
        });
    report(worktree, &finished)
}

/// Removes an unused worktree after a refused spawn (kept if anything is in it).
pub(crate) async fn discard(worktree: &HelperWorktree) {
    let _ = finish(worktree).await;
}

fn report(worktree: &HelperWorktree, finished: &Finished) -> Value {
    let note = if finished.removed {
        "The helper changed nothing, so its worktree and branch were removed.".to_string()
    } else if finished.inspection_failed {
        format!(
            "The worktree could not be inspected ({}); it was kept, so treat its contents as unknown. {}",
            finished.error.as_deref().unwrap_or("unknown error"),
            review_note(worktree)
        )
    } else {
        review_note(worktree)
    };
    let mut value = json!({
        "path": worktree.display_path,
        "branch": worktree.worktree.branch,
        "baseCommit": worktree.worktree.base_commit,
        "removed": finished.removed,
        "note": bounded(&note, MAX_NOTE_BYTES),
    });
    if finished.inspection_failed {
        value["inspectionFailed"] = json!(true);
    }
    if let Some(outcome) = &finished.outcome {
        value["changedFiles"] = json!(outcome
            .changed_files
            .iter()
            .take(MAX_SUMMARY_FILES)
            .map(|file| json!({"path": file.path, "status": file.status}))
            .collect::<Vec<_>>());
        value["changedFilesTotal"] = json!(outcome.changed_files_total);
        value["commitsAhead"] = json!(outcome.commits_ahead);
        value["diffStat"] = json!(bounded(&outcome.diff_stat, MAX_SUMMARY_DIFF_STAT));
    }
    value
}

fn review_note(worktree: &HelperWorktree) -> String {
    let path = &worktree.display_path;
    let branch = &worktree.worktree.branch;
    format!(
        "Review with `git -C {path} status` and `git -C {path} diff`. To keep the work, commit it in the worktree and `git merge {branch}`, or apply it with `git -C {path} diff | git apply` (add untracked files separately). Remove it with `git worktree remove --force {path}` and `git branch -D {branch}`."
    )
}

/// Appended to the helper's task.
pub(crate) fn task_note(worktree: &HelperWorktree) -> String {
    format!(
        "\n\n[Worktree] You are working in an isolated copy of the repository at {} on branch {}. Your file tools already point there; edit files only there. You cannot run git. The companion reviews and merges your changes, so describe what you changed.",
        worktree.display_path, worktree.worktree.branch
    )
}

/// The daemon-owned config entries of a worktree assignment.
pub(crate) fn config_entries(worktree: &HelperWorktree) -> [(String, DataValue); 2] {
    let text = |path: &Path| DataValue::String(path.to_string_lossy().into_owned());
    [
        (ROOT_KEY.into(), text(&worktree.root)),
        (
            WORKTREE_KEY.into(),
            DataValue::Object(std::collections::BTreeMap::from([
                ("path".into(), text(&worktree.worktree.path)),
                (
                    "branch".into(),
                    DataValue::String(worktree.worktree.branch.clone()),
                ),
                (
                    "base".into(),
                    DataValue::String(worktree.worktree.base_commit.clone()),
                ),
            ])),
        ),
    ]
}

/// The tool root of a worktree helper's run, if its config has one. Only a
/// helper config whose root lies in an `anima-helper-*` worktree qualifies.
pub(crate) fn run_root(config: &AgentConfig) -> Option<PathBuf> {
    if !super::is_helper_config(config) {
        return None;
    }
    let additional = &config.settings.as_ref()?.additional;
    let DataValue::String(root) = additional.get(ROOT_KEY)? else {
        return None;
    };
    let DataValue::Object(worktree) = additional.get(WORKTREE_KEY)? else {
        return None;
    };
    let DataValue::String(path) = worktree.get("path")? else {
        return None;
    };
    let (root, path) = (PathBuf::from(root), PathBuf::from(path));
    let is_helper_tree = path.is_absolute()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(anima_worktree::NAME_PREFIX))
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == anima_worktree::WORKTREE_DIR);
    (is_helper_tree
        && root.starts_with(&path)
        && !root.components().any(|part| part == Component::ParentDir))
    .then_some(root)
}

/// Sweeps stale helper worktrees of the workspace repository and of every
/// remembered repository, sparing those of unfinished async assignments.
pub(crate) async fn startup_sweep(state: crate::app::SharedDaemonState) {
    let (repos, live) = {
        let guard = state.read().await;
        let mut repos: Vec<PathBuf> = guard
            .workspace
            .as_ref()
            .map(|workspace| workspace.root_path.clone())
            .into_iter()
            .collect();
        repos.extend(guard.helper_worktree_repos.iter().map(PathBuf::from));
        let live: Vec<PathBuf> = guard
            .runs
            .active_records()
            .into_iter()
            .filter_map(|record| record.async_helper.as_ref()?.worktree.as_ref())
            .map(|worktree| worktree.worktree.path.clone())
            .collect();
        (repos, live)
    };
    let swept = tokio::task::spawn_blocking(move || {
        let worktrees = Worktrees::new();
        let mut seen = Vec::new();
        for repo in repos {
            let Ok(root) = worktrees.repo_root(&repo) else {
                continue;
            };
            if seen.contains(&root) {
                continue;
            }
            match worktrees.sweep(&root, &live) {
                Ok(report) if !report.removed.is_empty() || !report.errors.is_empty() => {
                    tracing::info!(repo = %root.display(), removed = report.removed.len(), kept = report.kept.len(), errors = ?report.errors, "swept helper worktrees");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(repo = %root.display(), %error, "helper worktree sweep failed")
                }
            }
            seen.push(root);
        }
    })
    .await;
    if swept.is_err() {
        tracing::warn!("helper worktree sweep stopped unexpectedly");
    }
}

/// Remembers the repository of a new worktree for later sweeps.
pub(crate) fn remember_repo(repos: &mut Vec<String>, worktree: &HelperWorktree) {
    let repo = worktree.worktree.repo_root.to_string_lossy().into_owned();
    if !repos.contains(&repo) {
        if repos.len() >= MAX_REMEMBERED_REPOS {
            repos.remove(0);
        }
        repos.push(repo);
    }
}

fn bounded(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}
