//! The post-update hand-off, against a temporary per-user folder. Nothing here
//! touches the installed binary or re-executes the test process: the swap is
//! `swap_at` on a file in the temporary folder, and the rollback is recorded.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::handoff::{
    install, on_start, roll_back, rolled_back_message, HandoffOutcome, PendingUpdate,
};
use super::{swap_at, UpdateError};
use crate::paths::AnimaPaths;

fn folder() -> (tempfile::TempDir, AnimaPaths) {
    let home = tempfile::tempdir().unwrap();
    let paths = AnimaPaths::from_home(home.path());
    paths.ensure().unwrap();
    (home, paths)
}

fn pending_file(paths: &AnimaPaths) -> PathBuf {
    paths.run.join("update-pending.json")
}

fn failed_file(paths: &AnimaPaths) -> PathBuf {
    paths.run.join("update-failed.json")
}

fn write_json(path: &Path, value: Value) {
    fs::write(path, value.to_string()).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn pending_to(paths: &AnimaPaths, to: &str) -> PendingUpdate {
    PendingUpdate {
        from: "0.1.0".into(),
        to: to.into(),
        old: paths.root.join("bin/anima.old"),
    }
}

#[test]
fn pending_matching_version_waits_for_health() {
    let (_home, paths) = folder();
    let pending = pending_to(&paths, "0.2.0");
    write_json(
        &pending_file(&paths),
        json!({ "from": "0.1.0", "to": "0.2.0", "old": pending.old }),
    );

    assert_eq!(
        on_start(&paths, "0.2.0"),
        HandoffOutcome::AwaitHealth(pending)
    );
    // Kept until the server is confirmed running (or rolled back).
    assert!(pending_file(&paths).exists());
}

#[test]
fn pending_other_version_is_ignored_and_removed() {
    let (_home, paths) = folder();
    write_json(
        &pending_file(&paths),
        json!({ "from": "0.1.0", "to": "0.2.0", "old": "/x/anima.old" }),
    );
    // The old binary came back some other way: nothing to finish.
    assert_eq!(on_start(&paths, "0.1.0"), HandoffOutcome::Nothing);
    assert!(!pending_file(&paths).exists());

    // An unreadable one is dropped too.
    fs::write(pending_file(&paths), "{not json").unwrap();
    assert_eq!(on_start(&paths, "0.1.0"), HandoffOutcome::Nothing);
    assert!(!pending_file(&paths).exists());

    // And with no file there is nothing to do.
    assert_eq!(on_start(&paths, "0.1.0"), HandoffOutcome::Nothing);
}

#[test]
fn health_timeout_triggers_rollback() {
    let (_home, paths) = folder();
    let pending = pending_to(&paths, "0.2.0");
    write_json(
        &pending_file(&paths),
        serde_json::to_value(&pending).unwrap(),
    );
    let exe = paths.root.join("bin/anima");
    let calls = RefCell::new(Vec::new());

    roll_back(
        &paths,
        &pending,
        &exe,
        "The server was not running within 60 seconds",
        |current: &Path, old: &Path| {
            calls
                .borrow_mut()
                .push((current.to_path_buf(), old.to_path_buf()));
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(calls.into_inner(), [(exe, pending.old.clone())]);
    assert_eq!(
        read_json(&failed_file(&paths)),
        json!({ "to": "0.2.0", "error": "The server was not running within 60 seconds" })
    );
    assert!(!pending_file(&paths).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(failed_file(&paths))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn failed_rollback_leaves_no_marker_and_no_pending_file() {
    let (_home, paths) = folder();
    let pending = pending_to(&paths, "0.2.0");
    write_json(
        &pending_file(&paths),
        serde_json::to_value(&pending).unwrap(),
    );
    let outcome = roll_back(&paths, &pending, Path::new("/x/anima"), "boom", |_, _| {
        Err(UpdateError::Io(std::io::Error::other("no .old")))
    });
    assert!(outcome.is_err());
    // Not retried on the next start, and the old binary is not told it was restored.
    assert!(!pending_file(&paths).exists());
    assert!(!failed_file(&paths).exists());
}

#[test]
fn failed_marker_surfaces_last_error() {
    let (_home, paths) = folder();
    write_json(
        &failed_file(&paths),
        json!({ "to": "0.2.0", "error": "The server was not running within 60 seconds" }),
    );
    let last_error = "Update to v0.2.0 failed to start; rolled back";
    assert_eq!(rolled_back_message("0.2.0"), last_error);
    assert_eq!(
        on_start(&paths, "0.1.0"),
        HandoffOutcome::RolledBack {
            last_error: last_error.to_string()
        }
    );
    // Shown once.
    assert!(!failed_file(&paths).exists());
    assert_eq!(on_start(&paths, "0.1.0"), HandoffOutcome::Nothing);
}

#[test]
fn install_swaps_and_records_the_pending_update() {
    let (_home, paths) = folder();
    let exe = paths.root.join("anima");
    fs::write(&exe, b"old binary").unwrap();

    let pending = install(&paths, &exe, b"new binary", "0.1.0", "0.2.0", |binary| {
        swap_at(binary, &exe)
    })
    .unwrap();

    let old = paths.root.join("anima.old");
    assert_eq!(fs::read(&exe).unwrap(), b"new binary");
    assert_eq!(fs::read(&old).unwrap(), b"old binary");
    assert_eq!(
        pending,
        PendingUpdate {
            from: "0.1.0".into(),
            to: "0.2.0".into(),
            old: old.clone(),
        }
    );
    assert_eq!(
        read_json(&pending_file(&paths)),
        json!({ "from": "0.1.0", "to": "0.2.0", "old": old })
    );
    // The new binary, started with this file, waits for its server.
    assert_eq!(
        on_start(&paths, "0.2.0"),
        HandoffOutcome::AwaitHealth(pending)
    );
}

#[test]
fn install_records_the_pending_update_before_the_swap() {
    let (_home, paths) = folder();
    let exe = paths.root.join("anima");
    fs::write(&exe, b"old binary").unwrap();
    let recorded = RefCell::new(None);

    install(&paths, &exe, b"new binary", "0.1.0", "0.2.0", |binary| {
        *recorded.borrow_mut() = Some(read_json(&pending_file(&paths)));
        swap_at(binary, &exe)
    })
    .unwrap();

    // Already there when the swap began, with the `.old` it would keep.
    assert_eq!(
        recorded.into_inner(),
        Some(json!({ "from": "0.1.0", "to": "0.2.0", "old": paths.root.join("anima.old") }))
    );
}

#[test]
fn install_that_cannot_record_does_not_swap() {
    let (_home, paths) = folder();
    let exe = paths.root.join("anima");
    fs::write(&exe, b"old binary").unwrap();
    // The run folder is a file, so nothing can be recorded in it.
    fs::remove_dir_all(&paths.run).unwrap();
    fs::write(&paths.run, b"not a folder").unwrap();
    let swapped = RefCell::new(false);

    let outcome = install(&paths, &exe, b"new binary", "0.1.0", "0.2.0", |_| {
        *swapped.borrow_mut() = true;
        Ok(paths.root.join("anima.old"))
    });

    assert!(outcome.is_err());
    assert!(!swapped.into_inner());
    assert_eq!(fs::read(&exe).unwrap(), b"old binary");
}

#[test]
fn install_that_cannot_swap_changes_nothing() {
    let (_home, paths) = folder();
    let exe = paths.root.join("missing/anima");
    let outcome = install(&paths, &exe, b"new", "0.1.0", "0.2.0", |binary| {
        // Recorded before the swap was tried…
        assert!(pending_file(&paths).exists());
        swap_at(binary, &exe)
    });
    assert!(outcome.is_err());
    // …and dropped once it failed.
    assert!(!pending_file(&paths).exists());
}
