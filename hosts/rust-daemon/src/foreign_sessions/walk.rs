//! Finding session logs on this computer. Only regular files inside a
//! source's root are candidates: symlinks that resolve outside it, symlinked
//! directories, and anything that is not a regular file are skipped. A
//! candidate is named by a handle, the hex SHA-256 of its resolved path, so
//! no route ever accepts or returns a path.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::ForeignSource;

/// How long one walk answers list and resolve requests.
const WALK_CACHE_TTL: Duration = Duration::from_secs(5);
/// Files considered per source, so a huge store cannot stall a request.
const MAX_WALK_FILES: usize = 20_000;
/// Codex keeps `sessions/YYYY/MM/DD/rollout-*.jsonl`; deeper is not a log.
const MAX_CODEX_DEPTH: usize = 6;

/// The session stores of each tool.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Roots {
    pub(crate) claude_code: Option<PathBuf>,
    pub(crate) codex: Option<PathBuf>,
}

impl Roots {
    /// `$CLAUDE_CONFIG_DIR/projects` else `~/.claude/projects`, and
    /// `$CODEX_HOME/sessions` else `~/.codex/sessions`. A blank override
    /// counts as unset.
    pub(crate) fn from_env() -> Self {
        let home = home_dir();
        let default =
            |parts: [&str; 2]| home.as_ref().map(|home| home.join(parts[0]).join(parts[1]));
        Self {
            claude_code: env_root("CLAUDE_CONFIG_DIR", "projects", home.as_deref())
                .or_else(|| default([".claude", "projects"])),
            codex: env_root("CODEX_HOME", "sessions", home.as_deref())
                .or_else(|| default([".codex", "sessions"])),
        }
    }

    fn root(&self, source: ForeignSource) -> Option<&Path> {
        match source {
            ForeignSource::ClaudeCode => self.claude_code.as_deref(),
            ForeignSource::Codex => self.codex.as_deref(),
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn env_root(name: &str, subdir: &str, home: Option<&Path>) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    let text = value.to_string_lossy();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let base = match (text.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if text == "~" => home?.to_path_buf(),
        _ => PathBuf::from(text),
    };
    Some(base.join(subdir))
}

/// One session log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) handle: String,
    pub(crate) source: ForeignSource,
    /// Resolved, inside its source's root.
    pub(crate) path: PathBuf,
    pub(crate) modified_ms: u64,
    pub(crate) size_bytes: u64,
}

/// The hex SHA-256 of a resolved path.
pub(crate) fn handle_of(path: &Path) -> String {
    Sha256::digest(path.to_string_lossy().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Every log of every source, newest first (then by handle, descending).
pub(crate) fn walk(roots: &Roots) -> Vec<Candidate> {
    let mut found = Vec::new();
    for source in ForeignSource::ALL {
        if let Some(root) = roots.root(source) {
            found.extend(walk_source(source, root));
        }
    }
    found.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| b.handle.cmp(&a.handle))
    });
    found
}

pub(crate) fn walk_source(source: ForeignSource, root: &Path) -> Vec<Candidate> {
    let Ok(root) = std::fs::canonicalize(root) else {
        return Vec::new();
    };
    if !root.is_dir() {
        return Vec::new();
    }
    let mut walker = Walker {
        source,
        root: &root,
        found: Vec::new(),
        seen: HashSet::new(),
    };
    match source {
        // `projects/*/*.jsonl`, not recursive: sub-agent logs live deeper.
        ForeignSource::ClaudeCode => {
            for project in read_dir(&root) {
                if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                for entry in read_dir(&project.path()) {
                    if walker.full() {
                        break;
                    }
                    if name_of(&entry).is_some_and(|name| name.ends_with(".jsonl")) {
                        walker.consider(&entry);
                    }
                }
            }
        }
        // `sessions/**/rollout-*.jsonl`, without following directory links.
        ForeignSource::Codex => {
            let mut stack = vec![(root.clone(), 0usize)];
            while let Some((dir, depth)) = stack.pop() {
                for entry in read_dir(&dir) {
                    if walker.full() {
                        break;
                    }
                    if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                        if depth < MAX_CODEX_DEPTH {
                            stack.push((entry.path(), depth + 1));
                        }
                    } else if name_of(&entry).is_some_and(|name| {
                        name.starts_with("rollout-") && name.ends_with(".jsonl")
                    }) {
                        walker.consider(&entry);
                    }
                }
            }
        }
    }
    walker.found
}

struct Walker<'a> {
    source: ForeignSource,
    root: &'a Path,
    found: Vec<Candidate>,
    seen: HashSet<String>,
}

impl Walker<'_> {
    fn full(&self) -> bool {
        self.found.len() >= MAX_WALK_FILES
    }

    fn consider(&mut self, entry: &std::fs::DirEntry) {
        let Ok(resolved) = std::fs::canonicalize(entry.path()) else {
            return;
        };
        if !resolved.starts_with(self.root) {
            return;
        }
        let Ok(metadata) = std::fs::metadata(&resolved) else {
            return;
        };
        if !metadata.is_file() {
            return;
        }
        let handle = handle_of(&resolved);
        if !self.seen.insert(handle.clone()) {
            return;
        }
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_millis() as u64);
        self.found.push(Candidate {
            handle,
            source: self.source,
            path: resolved,
            modified_ms,
            size_bytes: metadata.len(),
        });
    }
}

fn read_dir(dir: &Path) -> impl Iterator<Item = std::fs::DirEntry> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
}

fn name_of(entry: &std::fs::DirEntry) -> Option<String> {
    entry.file_name().to_str().map(str::to_string)
}

struct WalkCache {
    at: Instant,
    roots: Roots,
    candidates: Arc<Vec<Candidate>>,
}

fn cache() -> &'static Mutex<Option<WalkCache>> {
    static CACHE: OnceLock<Mutex<Option<WalkCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// The last walk of these roots when it is a few seconds old at most,
/// else a new one. `force` always walks again. The flag is true when the
/// list was walked by this call.
pub(crate) fn cached_walk(roots: &Roots, force: bool) -> (Arc<Vec<Candidate>>, bool) {
    if !force {
        let cached = cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = cached.as_ref() {
            if entry.roots == *roots && entry.at.elapsed() < WALK_CACHE_TTL {
                return (Arc::clone(&entry.candidates), false);
            }
        }
    }
    let candidates = Arc::new(walk(roots));
    *cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(WalkCache {
        at: Instant::now(),
        roots: roots.clone(),
        candidates: Arc::clone(&candidates),
    });
    (candidates, true)
}
