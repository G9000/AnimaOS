//! Git worktree isolation for helper agents.
//!
//! A helper that edits files can work in its own linked worktree of the
//! parent's repository instead of the shared checkout. This crate creates
//! those worktrees, inspects what a helper left behind, removes worktrees that
//! hold no work, and sweeps stale ones. It is host-agnostic: it only drives the
//! `git` CLI (through [`git`], which neutralizes hooks, fsmonitor, filters and
//! other repository-chosen commands) and knows nothing about agents or runs.
//!
//! Layout: `<toplevel>/.worktrees/anima-helper-<name>` on branch
//! `anima-helper/<name>`, created from the repository's current `HEAD`. The
//! `.worktrees/` directory is excluded through `<git-common-dir>/info/exclude`
//! so no tracked file (such as `.gitignore`) is ever modified.
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

mod git;

use git::{bounded, Git};

/// Directory under the repository top level that holds helper worktrees.
pub const WORKTREE_DIR: &str = ".worktrees";
/// Directory-name prefix of every helper worktree.
pub const NAME_PREFIX: &str = "anima-helper-";
/// Branch-name prefix of every helper worktree.
pub const BRANCH_PREFIX: &str = "anima-helper/";
/// Most changed files listed in one [`Outcome`].
pub const MAX_LISTED_FILES: usize = 200;
/// Most bytes of `git diff --stat` kept in one [`Outcome`].
pub const MAX_DIFF_STAT_BYTES: usize = 8 * 1024;
/// Default limit for one git call.
pub const DEFAULT_GIT_TIMEOUT: Duration = Duration::from_secs(30);

const EXCLUDE_LINE: &str = "/.worktrees/";

/// Serializes edits of `info/exclude` within this process.
static EXCLUDE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeError {
    /// The worktree name is not `[a-z0-9-]{1,40}`.
    InvalidName,
    /// The requested repository directory does not exist or is not a directory.
    NotADirectory(PathBuf),
    /// The directory is not inside a git work tree.
    NotARepo,
    /// The repository has no commit to branch from.
    NoCommits,
    /// The worktree directory already exists.
    AlreadyExists(PathBuf),
    /// `git` could not be started.
    GitUnavailable(String),
    /// A git call outlived its timeout and was killed.
    Timeout(String),
    /// A git call failed.
    Git {
        command: String,
        message: String,
    },
    /// Repository filter discovery failed, so git cannot run safely here.
    FilterDiscovery,
    /// The worktree record does not describe a helper worktree.
    NotAHelperWorktree,
    Io(String),
}

impl fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("worktree name must match [a-z0-9-]{1,40}"),
            Self::NotADirectory(path) => write!(f, "{} is not a directory", path.display()),
            Self::NotARepo => f.write_str("the directory is not inside a git repository"),
            Self::NoCommits => f.write_str(
                "the git repository has no commits yet; commit once before using a worktree",
            ),
            Self::AlreadyExists(path) => write!(f, "{} already exists", path.display()),
            Self::GitUnavailable(error) => write!(f, "git could not be started: {error}"),
            Self::Timeout(command) => write!(f, "`{command}` timed out"),
            Self::Git { command, message } => write!(f, "`{command}` failed: {message}"),
            Self::FilterDiscovery => {
                f.write_str("repository filter discovery failed; git was not run")
            }
            Self::NotAHelperWorktree => f.write_str("not a helper worktree"),
            Self::Io(error) => write!(f, "worktree I/O failed: {error}"),
        }
    }
}

impl std::error::Error for WorktreeError {}

impl From<std::io::Error> for WorktreeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// One helper worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub name: String,
    /// The repository top level the worktree was created from.
    pub repo_root: PathBuf,
    /// The worktree's own top level.
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
}

/// One path the helper changed, as reported by `git status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    /// `git status` short code (`M`, `A`, `D`, `R`, ...), `untracked`, or `ignored`.
    pub status: String,
}

/// What a helper left in its worktree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// At most [`MAX_LISTED_FILES`] entries, including untracked and ignored files.
    pub changed_files: Vec<ChangedFile>,
    /// Every changed path, including those not listed.
    pub changed_files_total: usize,
    /// Untracked paths (a subset of `changed_files`).
    pub untracked: Vec<String>,
    /// `git diff --stat` of tracked changes against the base commit.
    pub diff_stat: String,
    pub diff_stat_truncated: bool,
    /// Commits on the worktree branch after the base commit.
    pub commits_ahead: u32,
}

impl Outcome {
    /// No edits, no new files, and no commits.
    pub fn is_clean(&self) -> bool {
        self.changed_files_total == 0 && self.commits_ahead == 0
    }
}

/// The result of [`Worktrees::finish`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finished {
    /// The worktree and its branch are gone.
    pub removed: bool,
    /// A probe failed, so nothing is known about the work: the worktree was kept.
    pub inspection_failed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The result of [`Worktrees::sweep`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub removed: Vec<PathBuf>,
    pub kept: Vec<PathBuf>,
    pub errors: Vec<String>,
}

/// Creates, inspects, finishes and sweeps helper worktrees.
#[derive(Clone, Debug)]
pub struct Worktrees {
    git: Git,
}

impl Default for Worktrees {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `name` is a valid helper worktree name: `[a-z0-9-]{1,40}`.
pub fn valid_name(name: &str) -> bool {
    (1..=40).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

impl Worktrees {
    pub fn new() -> Self {
        Self::with_timeout(DEFAULT_GIT_TIMEOUT)
    }

    /// A manager whose git calls are each killed after `timeout`.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            git: Git { timeout },
        }
    }

    /// The canonical top level of the work tree containing `dir`.
    pub fn repo_root(&self, dir: &Path) -> Result<PathBuf, WorktreeError> {
        if !dir.is_dir() {
            return Err(WorktreeError::NotADirectory(dir.to_path_buf()));
        }
        let output = self.git.run(
            dir,
            &[],
            &[OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
        )?;
        let top = output.stdout_text();
        if !output.success || top.is_empty() {
            return Err(WorktreeError::NotARepo);
        }
        Ok(PathBuf::from(top).canonicalize()?)
    }

    /// Creates `<toplevel>/.worktrees/anima-helper-<name>` on a new branch
    /// `anima-helper/<name>` from the current `HEAD` of the repository
    /// containing `repo_dir`.
    pub fn create(&self, repo_dir: &Path, name: &str) -> Result<Worktree, WorktreeError> {
        if !valid_name(name) {
            return Err(WorktreeError::InvalidName);
        }
        let repo_root = self.repo_root(repo_dir)?;
        let base = self.git.run(
            &repo_root,
            &[],
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new("--quiet"),
                OsStr::new("HEAD^{commit}"),
            ],
        )?;
        let base_commit = base.stdout_text();
        if !base.success || base_commit.is_empty() {
            return Err(WorktreeError::NoCommits);
        }
        self.ensure_excluded(&repo_root)?;
        let container = repo_root.join(WORKTREE_DIR);
        std::fs::create_dir_all(&container)?;
        let path = container.join(format!("{NAME_PREFIX}{name}"));
        if path.exists() {
            return Err(WorktreeError::AlreadyExists(path));
        }
        let branch = format!("{BRANCH_PREFIX}{name}");
        let filters = self.git.filter_overrides(&repo_root)?;
        let output = self.git.run(
            &repo_root,
            &filters,
            &[
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--quiet"),
                OsStr::new("-b"),
                OsStr::new(&branch),
                path.as_os_str(),
                OsStr::new(&base_commit),
            ],
        )?;
        if !output.success {
            return Err(WorktreeError::Git {
                command: "git worktree add".into(),
                message: output.stderr,
            });
        }
        let path = path.canonicalize()?;
        Ok(Worktree {
            name: name.to_string(),
            repo_root,
            path,
            branch,
            base_commit,
        })
    }

    /// Adds `/.worktrees/` to `<git-common-dir>/info/exclude` once.
    fn ensure_excluded(&self, repo_root: &Path) -> Result<(), WorktreeError> {
        let common = self
            .git
            .text(repo_root, &[], &["rev-parse", "--git-common-dir"])?;
        let common = PathBuf::from(common);
        let common = if common.is_absolute() {
            common
        } else {
            repo_root.join(common)
        };
        let info = common.join("info");
        let _guard = EXCLUDE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        std::fs::create_dir_all(&info)?;
        let exclude = info.join("exclude");
        let existing = match std::fs::read(&exclude) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        let present = existing.lines().any(|line| {
            matches!(
                line.trim(),
                "/.worktrees/" | ".worktrees/" | "/.worktrees" | ".worktrees"
            )
        });
        if present {
            return Ok(());
        }
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&exclude)?;
        file.write_all(format!("{separator}{EXCLUDE_LINE}\n").as_bytes())?;
        Ok(())
    }

    /// Reports what was changed in `worktree` since its base commit.
    pub fn inspect(&self, worktree: &Worktree) -> Result<Outcome, WorktreeError> {
        check_helper_worktree(worktree)?;
        if !worktree.path.is_dir() {
            return Err(WorktreeError::NotADirectory(worktree.path.clone()));
        }
        let filters = self.git.filter_overrides(&worktree.path)?;
        let (changed_files, changed_files_total, untracked) =
            self.status(&worktree.path, &filters)?;
        let commits_ahead = self
            .git
            .text(
                &worktree.path,
                &[],
                &[
                    "rev-list",
                    "--count",
                    &format!("{}..HEAD", worktree.base_commit),
                ],
            )?
            .parse::<u32>()
            .map_err(|_| WorktreeError::Git {
                command: "git rev-list --count".into(),
                message: "non-numeric output".into(),
            })?;
        let stat = self.git.run(
            &worktree.path,
            &filters,
            &[
                OsStr::new("diff"),
                OsStr::new("--no-ext-diff"),
                OsStr::new("--no-textconv"),
                OsStr::new("--no-color"),
                OsStr::new("--stat"),
                OsStr::new(&worktree.base_commit),
                OsStr::new("--"),
            ],
        )?;
        if !stat.success {
            return Err(WorktreeError::Git {
                command: "git diff --stat".into(),
                message: stat.stderr,
            });
        }
        let full = String::from_utf8_lossy(&stat.stdout).trim_end().to_string();
        let diff_stat = bounded(&full, MAX_DIFF_STAT_BYTES);
        Ok(Outcome {
            diff_stat_truncated: stat.truncated || diff_stat.len() < full.len(),
            diff_stat,
            changed_files,
            changed_files_total,
            untracked,
            commits_ahead,
        })
    }

    /// `git status` of `dir`, including untracked and ignored files: a fresh
    /// checkout has neither, so any present was created after it.
    fn status(
        &self,
        dir: &Path,
        filters: &[String],
    ) -> Result<(Vec<ChangedFile>, usize, Vec<String>), WorktreeError> {
        let output = self.git.run(
            dir,
            filters,
            &[
                OsStr::new("status"),
                OsStr::new("--porcelain=v1"),
                OsStr::new("-z"),
                OsStr::new("--untracked-files=all"),
                OsStr::new("--ignored=matching"),
            ],
        )?;
        if !output.success {
            return Err(WorktreeError::Git {
                command: "git status".into(),
                message: output.stderr,
            });
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut entries = text.split('\0').filter(|entry| !entry.is_empty());
        let mut files = Vec::new();
        let mut untracked = Vec::new();
        let mut total = 0usize;
        while let Some(entry) = entries.next() {
            if entry.len() < 4 || !entry.is_char_boundary(2) {
                continue;
            }
            let (code, path) = (&entry[..2], &entry[3..]);
            if code.starts_with('R') || code.starts_with('C') {
                // `-z` puts a rename's source path in the next field.
                entries.next();
            }
            total += 1;
            let status = match code {
                "??" => "untracked".to_string(),
                "!!" => "ignored".to_string(),
                other => other.trim().to_string(),
            };
            if code == "??" && untracked.len() < MAX_LISTED_FILES {
                untracked.push(path.to_string());
            }
            if files.len() < MAX_LISTED_FILES {
                files.push(ChangedFile {
                    path: path.to_string(),
                    status,
                });
            }
        }
        if output.truncated {
            // The list is incomplete; there is at least one more entry.
            total += 1;
        }
        Ok((files, total, untracked))
    }

    /// Inspects `worktree` and removes it with its branch when it holds no
    /// work. Any failed probe keeps it and sets `inspection_failed`.
    pub fn finish(&self, worktree: &Worktree) -> Finished {
        if let Err(error) = check_helper_worktree(worktree) {
            return Finished {
                inspection_failed: true,
                error: Some(error.to_string()),
                ..Finished::default()
            };
        }
        if !worktree.path.exists() {
            // Nothing on disk to review; forget the stale registration.
            let _ = self.git.run(
                &worktree.repo_root,
                &[],
                &[OsStr::new("worktree"), OsStr::new("prune")],
            );
            return Finished {
                removed: true,
                ..Finished::default()
            };
        }
        let outcome = match self.inspect(worktree) {
            Ok(outcome) => outcome,
            Err(error) => {
                return Finished {
                    inspection_failed: true,
                    error: Some(error.to_string()),
                    ..Finished::default()
                }
            }
        };
        if !outcome.is_clean() {
            return Finished {
                outcome: Some(outcome),
                ..Finished::default()
            };
        }
        match self.remove(&worktree.repo_root, &worktree.path, &worktree.branch) {
            Ok(()) => Finished {
                removed: true,
                outcome: Some(outcome),
                ..Finished::default()
            },
            Err(error) => Finished {
                outcome: Some(outcome),
                error: Some(error.to_string()),
                ..Finished::default()
            },
        }
    }

    fn remove(&self, repo_root: &Path, path: &Path, branch: &str) -> Result<(), WorktreeError> {
        let output = self.git.run(
            repo_root,
            &[],
            &[
                OsStr::new("worktree"),
                OsStr::new("remove"),
                OsStr::new("--force"),
                path.as_os_str(),
            ],
        )?;
        if !output.success {
            return Err(WorktreeError::Git {
                command: "git worktree remove".into(),
                message: output.stderr,
            });
        }
        if branch.starts_with(BRANCH_PREFIX) {
            self.git
                .text(repo_root, &[], &["branch", "-D", "--quiet", branch])?;
        }
        Ok(())
    }

    /// Prunes stale registrations, then removes every helper worktree of the
    /// repository containing `repo_dir` that is clean, has no commits missing
    /// from the repository's `HEAD`, is not locked, and is not in `live`.
    pub fn sweep(&self, repo_dir: &Path, live: &[PathBuf]) -> Result<SweepReport, WorktreeError> {
        let repo_root = self.repo_root(repo_dir)?;
        self.git.text(&repo_root, &[], &["worktree", "prune"])?;
        let head = self.git.run(
            &repo_root,
            &[],
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new("--quiet"),
                OsStr::new("HEAD^{commit}"),
            ],
        )?;
        if !head.success {
            return Ok(SweepReport::default());
        }
        let head = head.stdout_text();
        let listing = self.git.run(
            &repo_root,
            &[],
            &[
                OsStr::new("worktree"),
                OsStr::new("list"),
                OsStr::new("--porcelain"),
                OsStr::new("-z"),
            ],
        )?;
        if !listing.success {
            return Err(WorktreeError::Git {
                command: "git worktree list".into(),
                message: listing.stderr,
            });
        }
        let live: Vec<PathBuf> = live
            .iter()
            .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
            .collect();
        let mut report = SweepReport::default();
        for entry in parse_worktree_list(&String::from_utf8_lossy(&listing.stdout)) {
            let path = PathBuf::from(&entry.path);
            let is_helper = path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.strip_prefix(NAME_PREFIX).is_some_and(valid_name))
                && path
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|parent| parent == WORKTREE_DIR);
            if !is_helper {
                continue;
            }
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if entry.locked || live.contains(&canonical) || !path.is_dir() {
                report.kept.push(path);
                continue;
            }
            match self.sweepable(&path, &head, entry.head.as_deref()) {
                Ok(true) => {
                    let branch = entry
                        .branch
                        .as_deref()
                        .and_then(|branch| branch.strip_prefix("refs/heads/"))
                        .unwrap_or_default()
                        .to_string();
                    match self.remove(&repo_root, &path, &branch) {
                        Ok(()) => report.removed.push(path),
                        Err(error) => {
                            report.errors.push(error.to_string());
                            report.kept.push(path);
                        }
                    }
                }
                Ok(false) => report.kept.push(path),
                Err(error) => {
                    report.errors.push(error.to_string());
                    report.kept.push(path);
                }
            }
        }
        Ok(report)
    }

    fn sweepable(
        &self,
        path: &Path,
        repo_head: &str,
        worktree_head: Option<&str>,
    ) -> Result<bool, WorktreeError> {
        let Some(worktree_head) = worktree_head else {
            return Ok(false);
        };
        let filters = self.git.filter_overrides(path)?;
        let (_, changed, _) = self.status(path, &filters)?;
        if changed > 0 {
            return Ok(false);
        }
        let unique = self.git.text(
            path,
            &[],
            &[
                "rev-list",
                "--count",
                &format!("{repo_head}..{worktree_head}"),
            ],
        )?;
        Ok(unique == "0")
    }
}

fn check_helper_worktree(worktree: &Worktree) -> Result<(), WorktreeError> {
    let expected = worktree
        .repo_root
        .join(WORKTREE_DIR)
        .join(format!("{NAME_PREFIX}{}", worktree.name));
    let same_path = worktree.path == expected
        || expected
            .canonicalize()
            .is_ok_and(|canonical| canonical == worktree.path);
    if !valid_name(&worktree.name)
        || !same_path
        || worktree.branch != format!("{BRANCH_PREFIX}{}", worktree.name)
        || worktree.base_commit.is_empty()
        || !worktree
            .base_commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(WorktreeError::NotAHelperWorktree);
    }
    Ok(())
}

struct ListedWorktree {
    path: String,
    head: Option<String>,
    branch: Option<String>,
    locked: bool,
}

fn parse_worktree_list(text: &str) -> Vec<ListedWorktree> {
    let mut entries = Vec::new();
    let mut current: Option<ListedWorktree> = None;
    for field in text.split('\0') {
        if field.is_empty() {
            entries.extend(current.take());
            continue;
        }
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        match key {
            "worktree" => {
                entries.extend(current.take());
                current = Some(ListedWorktree {
                    path: value.to_string(),
                    head: None,
                    branch: None,
                    locked: false,
                });
            }
            "HEAD" => {
                if let Some(entry) = current.as_mut() {
                    entry.head = Some(value.to_string());
                }
            }
            "branch" => {
                if let Some(entry) = current.as_mut() {
                    entry.branch = Some(value.to_string());
                }
            }
            "locked" => {
                if let Some(entry) = current.as_mut() {
                    entry.locked = true;
                }
            }
            _ => {}
        }
    }
    entries.extend(current);
    entries
}

#[cfg(test)]
mod tests;
