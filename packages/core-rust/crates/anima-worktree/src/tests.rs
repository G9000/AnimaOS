use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

/// Test-only git with an isolated identity; never the hardened runner.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn repo() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "--quiet"]);
    std::fs::write(root.join("README.md"), "hello\n").unwrap();
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "--quiet", "-m", "init"]);
    let root = root.canonicalize().unwrap();
    (temp, root)
}

fn exclude_lines(root: &Path) -> usize {
    std::fs::read_to_string(root.join(".git/info/exclude"))
        .unwrap()
        .lines()
        .filter(|line| line.trim() == "/.worktrees/")
        .count()
}

#[test]
fn names_are_validated() {
    assert!(valid_name("ab12-cd"));
    assert!(!valid_name(""));
    assert!(!valid_name("Upper"));
    assert!(!valid_name("../x"));
    assert!(!valid_name(&"a".repeat(41)));
    let (_temp, root) = repo();
    assert_eq!(
        Worktrees::new().create(&root, "bad/name"),
        Err(WorktreeError::InvalidName)
    );
}

#[test]
fn clean_worktree_is_created_inspected_and_removed() {
    let (_temp, root) = repo();
    let worktrees = Worktrees::new();
    let worktree = worktrees.create(&root, "clean1").unwrap();
    assert_eq!(
        worktree.path,
        root.join(".worktrees/anima-helper-clean1")
            .canonicalize()
            .unwrap()
    );
    assert_eq!(worktree.branch, "anima-helper/clean1");
    assert_eq!(worktree.base_commit, git(&root, &["rev-parse", "HEAD"]));
    assert!(worktree.path.join("README.md").is_file());
    // The main checkout stays clean: `.worktrees/` is excluded, not ignored
    // through a tracked file.
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    assert!(!root.join(".gitignore").exists());

    let outcome = worktrees.inspect(&worktree).unwrap();
    assert!(outcome.is_clean(), "{outcome:?}");

    let finished = worktrees.finish(&worktree);
    assert!(finished.removed, "{finished:?}");
    assert!(!finished.inspection_failed);
    assert!(!worktree.path.exists());
    assert_eq!(git(&root, &["branch", "--list", "anima-helper/*"]), "");
}

#[test]
fn edited_worktree_is_kept_and_lists_changes() {
    let (_temp, root) = repo();
    let worktrees = Worktrees::new();
    let worktree = worktrees.create(&root, "edit1").unwrap();
    std::fs::write(worktree.path.join("README.md"), "changed\n").unwrap();
    std::fs::create_dir(worktree.path.join("src")).unwrap();
    std::fs::write(worktree.path.join("src/new.rs"), "fn main() {}\n").unwrap();

    let finished = worktrees.finish(&worktree);
    assert!(!finished.removed);
    assert!(!finished.inspection_failed);
    let outcome = finished.outcome.unwrap();
    assert_eq!(outcome.changed_files_total, 2);
    assert!(outcome.changed_files.contains(&ChangedFile {
        path: "README.md".into(),
        status: "M".into()
    }));
    assert!(outcome.changed_files.contains(&ChangedFile {
        path: "src/new.rs".into(),
        status: "untracked".into()
    }));
    assert_eq!(outcome.untracked, vec!["src/new.rs".to_string()]);
    assert!(
        outcome.diff_stat.contains("README.md"),
        "{}",
        outcome.diff_stat
    );
    assert_eq!(outcome.commits_ahead, 0);
    assert!(worktree.path.is_dir());
    // The main checkout never saw the edit.
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "hello\n"
    );
}

#[test]
fn commits_in_the_worktree_keep_it() {
    let (_temp, root) = repo();
    let worktrees = Worktrees::new();
    let worktree = worktrees.create(&root, "commit1").unwrap();
    std::fs::write(worktree.path.join("a.txt"), "a\n").unwrap();
    git(&worktree.path, &["add", "a.txt"]);
    git(&worktree.path, &["commit", "--quiet", "-m", "work"]);
    let finished = worktrees.finish(&worktree);
    assert!(!finished.removed);
    assert_eq!(finished.outcome.unwrap().commits_ahead, 1);
}

#[test]
fn ignored_files_written_by_a_helper_keep_the_worktree() {
    let (_temp, root) = repo();
    std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "ignore"]);
    let worktrees = Worktrees::new();
    let worktree = worktrees.create(&root, "ignored1").unwrap();
    std::fs::create_dir(worktree.path.join("build")).unwrap();
    std::fs::write(worktree.path.join("build/out.txt"), "x").unwrap();
    let finished = worktrees.finish(&worktree);
    assert!(!finished.removed, "{finished:?}");
    assert_eq!(
        finished.outcome.unwrap().changed_files[0].status,
        "ignored".to_string()
    );
}

#[test]
fn not_a_repo_and_no_commits_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let plain = temp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let worktrees = Worktrees::new();
    assert_eq!(worktrees.create(&plain, "x1"), Err(WorktreeError::NotARepo));
    assert!(matches!(
        worktrees.create(&temp.path().join("missing"), "x1"),
        Err(WorktreeError::NotADirectory(_))
    ));

    let empty = temp.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    git(&empty, &["init", "--quiet"]);
    assert_eq!(
        worktrees.create(&empty, "x1"),
        Err(WorktreeError::NoCommits)
    );
    assert!(!empty.join(".worktrees").exists());
}

#[test]
fn exclude_entry_is_added_once() {
    let (_temp, root) = repo();
    let worktrees = Worktrees::new();
    let first = worktrees.create(&root, "one").unwrap();
    let second = worktrees.create(&root, "two").unwrap();
    assert_eq!(exclude_lines(&root), 1);
    assert!(worktrees.finish(&first).removed);
    assert!(worktrees.finish(&second).removed);
    worktrees.create(&root, "three").unwrap();
    assert_eq!(exclude_lines(&root), 1);
}

#[test]
fn an_existing_worktree_directory_is_refused() {
    let (_temp, root) = repo();
    let worktrees = Worktrees::new();
    worktrees.create(&root, "dup").unwrap();
    assert!(matches!(
        worktrees.create(&root, "dup"),
        Err(WorktreeError::AlreadyExists(_))
    ));
}

#[test]
fn creating_from_a_subdirectory_uses_the_top_level() {
    let (_temp, root) = repo();
    std::fs::create_dir(root.join("sub")).unwrap();
    let worktree = Worktrees::new().create(&root.join("sub"), "sub1").unwrap();
    assert_eq!(worktree.repo_root, root);
    assert!(worktree.path.starts_with(root.join(".worktrees")));
}

#[test]
fn sweep_removes_clean_stale_worktrees_and_keeps_dirty_or_live_ones() {
    let (_temp, root) = repo();
    let worktrees = Worktrees::new();
    let stale = worktrees.create(&root, "stale").unwrap();
    let dirty = worktrees.create(&root, "dirty").unwrap();
    let live = worktrees.create(&root, "live").unwrap();
    let committed = worktrees.create(&root, "committed").unwrap();
    std::fs::write(dirty.path.join("README.md"), "edited\n").unwrap();
    std::fs::write(committed.path.join("c.txt"), "c\n").unwrap();
    git(&committed.path, &["add", "c.txt"]);
    git(&committed.path, &["commit", "--quiet", "-m", "c"]);

    let report = worktrees
        .sweep(&root, std::slice::from_ref(&live.path))
        .unwrap();
    assert_eq!(report.removed, vec![stale.path.clone()], "{report:?}");
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(!stale.path.exists());
    assert!(dirty.path.is_dir() && live.path.is_dir() && committed.path.is_dir());
    let branches = git(&root, &["branch", "--list", "anima-helper/*"]);
    assert!(!branches.contains("anima-helper/stale"), "{branches}");
    assert!(branches.contains("anima-helper/dirty"));

    // Once the committed work is merged, the worktree holds nothing unique.
    git(
        &root,
        &["merge", "--quiet", "--ff-only", "anima-helper/committed"],
    );
    let report = worktrees
        .sweep(&root, std::slice::from_ref(&live.path))
        .unwrap();
    assert_eq!(report.removed, vec![committed.path.clone()], "{report:?}");
}

#[test]
fn sweep_prunes_registrations_of_deleted_worktrees() {
    let (_temp, root) = repo();
    let worktrees = Worktrees::new();
    let gone = worktrees.create(&root, "gone").unwrap();
    std::fs::remove_dir_all(&gone.path).unwrap();
    let report = worktrees.sweep(&root, &[]).unwrap();
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(!git(&root, &["worktree", "list"]).contains("anima-helper-gone"));
    let finished = worktrees.finish(&gone);
    assert!(finished.removed);
}

#[test]
fn hostile_repository_config_runs_no_hooks_fsmonitor_or_filters() {
    let (temp, root) = repo();
    let markers = temp.path().join("markers");
    std::fs::create_dir(&markers).unwrap();
    let script = |name: &str| {
        let path = temp.path().join(format!("{name}.sh"));
        std::fs::write(
            &path,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", markers.join(name).display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    };
    // A tracked file routed through repository-named filter and diff drivers.
    std::fs::write(root.join(".gitattributes"), "*.txt filter=evil diff=evil\n").unwrap();
    std::fs::write(root.join("data.txt"), "data\n").unwrap();
    git(&root, &["add", ".gitattributes", "data.txt"]);
    git(&root, &["commit", "--quiet", "-m", "attrs"]);
    // Then the hostile configuration: hooks (default location and
    // core.hooksPath), fsmonitor, filters and diff drivers.
    let hook = script("hook");
    let hooks = root.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::copy(&hook, hooks.join("post-checkout")).unwrap();
    let custom_hooks = temp.path().join("custom-hooks");
    std::fs::create_dir(&custom_hooks).unwrap();
    std::fs::copy(&hook, custom_hooks.join("post-checkout")).unwrap();
    let filter = script("filter");
    for (key, value) in [
        ("core.hooksPath", custom_hooks.clone()),
        ("core.fsmonitor", script("fsmonitor")),
        ("filter.evil.smudge", filter.clone()),
        ("filter.evil.clean", filter.clone()),
        ("diff.evil.textconv", script("textconv")),
        ("diff.evil.command", script("diffcmd")),
    ] {
        git(&root, &["config", key, value.to_str().unwrap()]);
    }

    let worktrees = Worktrees::new();
    let worktree = worktrees.create(&root, "hostile").unwrap();
    std::fs::write(worktree.path.join("data.txt"), "edited\n").unwrap();
    let finished = worktrees.finish(&worktree);
    assert!(!finished.inspection_failed, "{finished:?}");
    assert!(!finished.removed);
    let ran: Vec<_> = std::fs::read_dir(&markers)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(ran.is_empty(), "repository-chosen commands ran: {ran:?}");
}

#[test]
fn finish_refuses_records_that_are_not_helper_worktrees() {
    let (_temp, root) = repo();
    let forged = Worktree {
        name: "x1".into(),
        repo_root: root.clone(),
        path: root.clone(),
        branch: "main".into(),
        base_commit: git(&root, &["rev-parse", "HEAD"]),
    };
    let finished = Worktrees::new().finish(&forged);
    assert!(!finished.removed && finished.inspection_failed);
    assert!(root.join("README.md").is_file());
}

#[test]
fn a_timed_out_call_is_reported() {
    let (_temp, root) = repo();
    let worktrees = Worktrees::with_timeout(std::time::Duration::from_nanos(1));
    assert!(matches!(
        worktrees.repo_root(&root),
        Err(WorktreeError::Timeout(_))
    ));
}
